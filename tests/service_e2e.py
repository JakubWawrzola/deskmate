"""End-to-end test of deskmate-service against the fake hub.

Not a pytest module: it starts real processes and, in one mode, installs a
Windows service. Run it by hand or from CI:

    python tests/service_e2e.py --console [--wsl Ubuntu-24.04]
    python tests/service_e2e.py --service

--console starts the service in the foreground with a temporary data folder,
a key file and pipes of its own. It touches nothing outside that folder and
needs no administrator. With --wsl the fake hub runs inside that WSL
distribution, for a Windows Python without aiohttp and cryptography.

--service is for CI. It installs the real service as LocalSystem, sets it up
through the admin pipe, and at the end uninstalls it and checks that nothing
is left. It needs an elevated terminal. Do not run it on a computer you use.

Both modes need a binary built with `--features test-hooks`: power commands
are then recorded, never run.
"""

from __future__ import annotations

import argparse
import base64
import ctypes
import hashlib
import hmac
import json
import os
import queue
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from multiprocessing.connection import Listener
from pathlib import Path
from typing import IO, Any

REPO = Path(__file__).resolve().parent.parent
NODE = "test_pc"
SERVICE = "DeskmateService"
STATUS_PIPE = "deskmate-service-status"
ADMIN_PIPE = "deskmate-service-admin"
WAIT_S = 45
PUBLISH_INTERVAL_S = 2
# The service refuses power commands this long after boot or a wake-up.
QUIET_AFTER_START_S = 300
# Longer than the 10 s the service waits for the hub's `info`.
NO_INFO_WAIT_S = 25

SYNCHRONIZE = 0x00100000
MUTEX_MODIFY_STATE = 0x0001
ERROR_ACCESS_DENIED = 5
ERROR_ALREADY_EXISTS = 183
ERROR_SERVICE_DOES_NOT_EXIST = 1060


class Hub:
    """The fake hub as a child process, driven through its stdin and stdout."""

    def __init__(self, command: list[str]) -> None:
        self._process = subprocess.Popen(  # noqa: S603 - the fake hub of this repository, started with arguments built here
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        self._events: queue.Queue[dict[str, Any]] = queue.Queue()
        threading.Thread(target=self._pump, daemon=True).start()
        listening = self.event()
        assert listening["event"] == "listening", listening
        self.port: int = listening["port"]

    def _pump(self) -> None:
        assert self._process.stdout is not None
        for line in self._process.stdout:
            if line.strip():
                self._events.put(json.loads(line))

    def event(self, timeout: float = WAIT_S) -> dict[str, Any]:
        try:
            return self._events.get(timeout=timeout)
        except queue.Empty:
            raise AssertionError("the hub saw nothing in time") from None

    def nothing(self, seconds: float) -> None:
        """Nothing reaches the hub for this long."""
        try:
            event = self._events.get(timeout=seconds)
        except queue.Empty:
            return
        raise AssertionError(f"expected nothing, the hub saw {event}")

    def session(self) -> int:
        event = self.event()
        assert event["event"] == "session", event
        assert event["node"] == NODE, event
        return event["n"]

    def frame(self) -> dict[str, Any]:
        """The next thing the hub sees, which must be a frame."""
        event = self.event()
        assert event["event"] == "frame", event
        return event["frame"]

    def frame_of(self, kind: str) -> dict[str, Any]:
        """The next frame of this type; states and pings may come before."""
        while True:
            frame = self.frame()
            if frame["t"] == kind:
                return frame
            assert frame["t"] in ("state", "ping"), frame

    def closed(self, session: int, *, bye: str | None) -> None:
        """The session ends, after a `bye` with this reason or without one."""
        said: str | None = None
        while True:
            event = self.event()
            if event["event"] == "closed":
                assert event["n"] == session, event
                break
            frame = event["frame"]
            if frame["t"] == "bye":
                said = frame["reason"]
            else:
                assert frame["t"] in ("state", "ping"), frame
        assert said == bye, f"bye was {said}, expected {bye}"

    def drain(self) -> None:
        """Forgets what the hub has seen so far."""
        while not self._events.empty():
            self._events.get_nowait()

    def send(self, frame: dict[str, Any]) -> None:
        self.command({"send": frame})

    def command(self, command: dict[str, Any]) -> None:
        assert self._process.stdin is not None
        self._process.stdin.write(json.dumps(command) + "\n")
        self._process.stdin.flush()

    def quit(self) -> None:
        if self._process.poll() is None:
            try:
                self.command({"quit": True})
                self._process.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                self._process.kill()


def wsl_path(path: Path) -> str:
    """C:\\Users\\me\\file as WSL sees it: /mnt/c/Users/me/file."""
    resolved = path.resolve()
    drive = resolved.drive.rstrip(":").lower()
    return f"/mnt/{drive}/" + "/".join(resolved.parts[1:])


def start_hub(args: argparse.Namespace, key_file: Path, *, no_info: bool) -> Hub:
    options = ["--port", "0", "--node", NODE]
    if no_info:
        options.append("--no-info")
    if args.wsl:
        script = " ".join(
            [
                "cd",
                shlex.quote(wsl_path(REPO)),
                "&&",
                args.wsl_python,
                "scripts/fake_link_hub.py",
                "--key-file",
                shlex.quote(wsl_path(key_file)),
                "--host",
                "0.0.0.0",  # noqa: S104 - inside WSL, so that Windows reaches the hub on localhost
                *options,
            ]
        )
        return Hub(["wsl.exe", "-d", args.wsl, "--", "bash", "-c", script])
    script_path = REPO / "scripts" / "fake_link_hub.py"
    return Hub(
        [sys.executable, str(script_path), "--key-file", str(key_file), *options]
    )


def settings(port: int) -> dict[str, Any]:
    return {
        "node_id": NODE,
        "link_url": f"ws://localhost:{port}/api/deskmate_link/ws",
        "link_url_remote": "",
        "link_cascade": False,
        "publish_interval_secs": PUBLISH_INTERVAL_S,
        "device_name": "E2E test PC",
        "sensors_enabled": {"battery": False},
    }


def fingerprint(config: dict[str, Any], key: str) -> str:
    """The fingerprint as docs/dev/ARCHITECTURE.md describes it."""

    def flag(on: bool) -> bytes:
        return b"1" if on else b"0"

    sensors = config["sensors_enabled"]
    parts = [
        b"deskmate service config v1",
        config["node_id"].encode(),
        config["link_url"].encode(),
        config["link_url_remote"].encode(),
        flag(config["link_cascade"]),
        str(config["publish_interval_secs"]).encode(),
        config["device_name"].encode(),
        str(len(sensors)).encode(),
    ]
    for name in sorted(sensors):
        parts += [name.encode(), flag(sensors[name])]
    parts.append(b"")
    message = b"".join(len(part).to_bytes(4, "big") + part for part in parts)
    return hmac.new(base64.b64decode(key), message, hashlib.sha256).hexdigest()


def run(command: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(  # noqa: S603 - the service binary under test or a Windows tool, with arguments built here
        command, capture_output=True, text=True, check=False, timeout=180
    )
    if check and result.returncode != 0:
        raise AssertionError(
            f"{command} exited with {result.returncode}\n{result.stdout}\n{result.stderr}"
        )
    return result


def read_status(binary: Path, suffix: str | None) -> dict[str, Any]:
    command = [str(binary), "status"]
    if suffix:
        command += ["--pipe-suffix", suffix]
    status = json.loads(run(command).stdout)
    assert set(status) == {"v", "version", "state", "node_id", "detail", "config"}
    assert status["v"] == 1, status
    return status


def wait_for_state(binary: Path, suffix: str | None, state: str) -> dict[str, Any]:
    deadline = time.monotonic() + WAIT_S
    while True:
        status = read_status(binary, suffix)
        if status["state"] == state:
            return status
        assert time.monotonic() < deadline, (
            f"the state stayed {status}, expected {state}"
        )
        time.sleep(0.3)


def read_status_pipe(suffix: str | None) -> dict[str, Any]:
    """The status line read straight from the pipe, as any program would."""
    name = STATUS_PIPE + (f"-{suffix}" if suffix else "")
    with open(rf"\\.\pipe\{name}", "rb") as pipe:
        return json.loads(pipe.read())


def uptime_s() -> float:
    kernel32 = ctypes.WinDLL("kernel32")
    kernel32.GetTickCount64.restype = ctypes.c_uint64
    return kernel32.GetTickCount64() / 1000


class Seat:
    """The seat objects, used the way a Deskmate app uses them."""

    def __init__(self) -> None:
        self._kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        for function in ("CreateEventW", "CreateMutexW", "OpenMutexW"):
            getattr(self._kernel32, function).restype = ctypes.c_void_p
        self._kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        self._mutex = rf"Global\Deskmate.Seat.{NODE}"
        self._waiting = rf"Global\Deskmate.Seat.{NODE}.waiting"

    def close(self, handle: int) -> None:
        self._kernel32.CloseHandle(handle)

    def can_open(self) -> bool:
        """Open the seat to wait on it and release it, as the descriptor allows."""
        handle = self._kernel32.OpenMutexW(
            SYNCHRONIZE | MUTEX_MODIFY_STATE, False, self._mutex
        )
        if handle:
            self.close(handle)
        return bool(handle)

    def take(self) -> tuple[int | None, int]:
        """What an app does to take the seat: the handle, or None, and the error."""
        handle = self._kernel32.CreateMutexW(None, False, self._mutex)
        error = ctypes.get_last_error()
        if handle and error == ERROR_ALREADY_EXISTS:
            self.close(handle)
            return None, error
        return handle or None, error

    def announce_waiting(self) -> int:
        handle = self._kernel32.CreateEventW(None, True, False, self._waiting)
        assert handle, f"cannot announce waiting: error {ctypes.get_last_error()}"
        return handle


def check_session_start(hub: Hub) -> int:
    """A session opens: info first, then the machine declare, then states."""
    session = hub.session()
    info = hub.frame()
    assert info["t"] == "info", info
    assert info["role"] == "service", info
    assert info["platform"] == "windows", info
    # The hub answers info by itself; nothing else may come before declare.
    declare = hub.frame()
    assert declare["t"] == "declare", declare
    assert declare["scope"] == "machine", declare
    assert declare["device"]["name"] == "E2E test PC", declare
    keys = {entity["key"] for entity in declare["entities"]}
    assert {"cpu", "memory", "uptime"} <= keys, keys
    assert {"sleep", "hibernate", "shutdown", "restart"} <= keys, keys
    assert "battery" not in keys, "a sensor switched off in the settings"
    assert "lock" not in keys, "nothing of a user's session"
    state = hub.frame()
    assert state["t"] == "state", state
    assert "cpu" in state["s"], state
    return session


def check_states(hub: Hub) -> None:
    first = time.monotonic()
    hub.frame_of("state")
    hub.frame_of("state")
    elapsed = time.monotonic() - first
    assert elapsed < 4 * PUBLISH_INTERVAL_S + 5, f"two states took {elapsed:.1f} s"


def check_status(
    binary: Path, suffix: str | None, config: dict[str, Any], key: str
) -> None:
    status = wait_for_state(binary, suffix, "connected")
    assert status["node_id"] == NODE, status
    assert status["config"] == fingerprint(config, key), "the fingerprint differs"
    assert key not in json.dumps(status)
    assert str(config["link_url"]) not in json.dumps(status), "no address in the status"
    assert read_status_pipe(suffix)["state"] == "connected"


def check_restart_command(hub: Hub, security_log: Path) -> None:
    """A restart is acknowledged, recorded by the dry run, and nothing restarts."""
    boot_time = time.time() - uptime_s()
    command = {"t": "cmd", "id": 41, "key": "restart", "action": "press"}
    if uptime_s() < QUIET_AFTER_START_S - 5:
        # Soon after boot the valve is closed: see that, then wait it out.
        hub.send(command)
        ack = hub.frame_of("ack")
        assert ack["id"] == 41 and ack["ok"] is False, ack
        assert "five minutes" in ack["error"], ack
        assert "result=refused_after_start" in security_log.read_text(encoding="utf-8")
        time.sleep(max(0.0, QUIET_AFTER_START_S + 3 - uptime_s()))
    hub.send({**command, "id": 42})
    ack = hub.frame_of("ack")
    assert ack == {"t": "ack", "id": 42, "ok": True}, ack
    log = security_log.read_text(encoding="utf-8")
    assert "power_command key=restart executor=dry_run result=ok" in log, log
    assert "executor=system" not in log, (
        "the real executor must not exist in this build"
    )
    # A command that is not a power command is refused and never run.
    hub.send({"t": "cmd", "id": 43, "key": "lock", "action": "press"})
    ack = hub.frame_of("ack")
    assert ack["id"] == 43 and ack["ok"] is False, ack
    # Still here, and the system did not start again in the meantime.
    time.sleep(1)
    assert abs((time.time() - uptime_s()) - boot_time) < 5, "the computer restarted"


def check_reconnect(hub: Hub, session: int) -> int:
    hub.command({"close": True})
    hub.closed(session, bye=None)
    return check_session_start(hub)


def check_handover(hub: Hub, session: int, binary: Path, suffix: str | None) -> int:
    """An app that waits gets the connection; the service takes it back."""
    seat = Seat()
    assert seat.can_open(), "a user's app must be able to open the service's seat"
    handle, error = seat.take()
    assert handle is None, "the seat is held by the service"
    assert error in (ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS), error

    waiting = seat.announce_waiting()
    hub.closed(session, bye="handover")
    wait_for_state(binary, suffix, "waiting")
    # The app's next step: stop announcing and take the free seat.
    seat.close(waiting)
    handle, error = seat.take()
    assert handle is not None, f"the seat was not released: error {error}"
    hub.nothing(8)
    seat.close(handle)
    return check_session_start(hub)


def check_too_old(hub: Hub, binary: Path, suffix: str | None) -> None:
    """A hub that never answers info gets no declare."""
    session = hub.session()
    info = hub.frame()
    assert info["t"] == "info", info
    deadline = time.monotonic() + NO_INFO_WAIT_S
    while True:
        event = hub.event(timeout=max(1.0, deadline - time.monotonic()))
        if event["event"] == "closed":
            assert event["n"] == session, event
            break
        assert event["frame"]["t"] == "ping", f"unexpected before the close: {event}"
    status = wait_for_state(binary, suffix, "integration_too_old")
    assert status["node_id"] == NODE, status
    hub.nothing(5)


# ---- console mode ----


def console_mode(args: argparse.Namespace, binary: Path) -> None:
    work = Path(tempfile.mkdtemp(prefix="deskmate-service-e2e-"))
    suffix = f"e2e-{os.getpid()}"
    key = base64.b64encode(os.urandom(32)).decode()
    key_file = work / "pairing-key"
    key_file.write_text(key, encoding="utf-8")
    data = work / "data"
    data.mkdir()
    hubs: list[Hub] = []
    service: subprocess.Popen[str] | None = None
    log: IO[str] | None = None
    try:
        hub = start_hub(args, key_file, no_info=False)
        hubs.append(hub)
        config = settings(hub.port)
        (data / "config.json").write_text(json.dumps(config), encoding="utf-8")

        log = open(work / "stderr.log", "w", encoding="utf-8")
        service = subprocess.Popen(  # noqa: S603 - the service binary under test, with arguments built here
            [
                str(binary),
                "console",
                "--data-dir",
                str(data),
                "--key-file",
                str(key_file),
                "--dry-run-power",
                "--pipe-suffix",
                suffix,
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
        )
        assert service.stdin is not None and service.stdout is not None
        assert service.stdout.readline().strip() == "ready"

        def tell(word: str) -> None:
            assert service is not None and service.stdin is not None
            service.stdin.write(word + "\n")
            service.stdin.flush()

        step("session, info, machine declare, states")
        session = check_session_start(hub)
        check_states(hub)
        step("status pipe and fingerprint")
        check_status(binary, suffix, config, key)
        step("restart command with the dry-run executor")
        check_restart_command(hub, data / "security.log")
        step("reconnect after the hub closed the session")
        session = check_reconnect(hub, session)
        step("handover to a waiting app and back")
        session = check_handover(hub, session, binary, suffix)
        step("suspend and resume")
        tell("suspend")
        hub.closed(session, bye="suspend")
        hub.nothing(4)
        tell("resume")
        session = check_session_start(hub)

        step("a hub that never answers info")
        old_hub = start_hub(args, key_file, no_info=True)
        hubs.append(old_hub)
        (data / "config.json").write_text(
            json.dumps(settings(old_hub.port)), encoding="utf-8"
        )
        tell("reload")
        hub.closed(session, bye=None)
        check_too_old(old_hub, binary, suffix)

        step("the admin pipe: refusals, and a reload that brings the first hub back")
        check_admin_refusals(config, suffix)
        (data / "config.json").write_text(json.dumps(config), encoding="utf-8")
        assert admin_request({"v": 1, "op": "reload"}, suffix) == {"v": 1, "ok": True}
        session = check_session_start(hub)
        step("stop")
        tell("stop")
        hub.closed(session, bye=None)
        assert service.wait(timeout=WAIT_S) == 0
        assert key not in (data / "service.log").read_text(encoding="utf-8")
        assert key not in (data / "security.log").read_text(encoding="utf-8")
        assert key not in (data / "config.json").read_text(encoding="utf-8")
    except BaseException:
        show_tail(work / "stderr.log")
        raise
    finally:
        if service is not None and service.poll() is None:
            service.kill()
            service.wait(timeout=10)
        if log is not None:
            log.close()
        for hub in hubs:
            hub.quit()
        shutil.rmtree(work, ignore_errors=True)


# ---- service mode ----


def admin_request(
    frame: dict[str, Any] | bytes, suffix: str | None = None
) -> dict[str, Any] | None:
    """Sends one line to the admin pipe; the answer, or None when it closed."""
    line = frame if isinstance(frame, bytes) else json.dumps(frame).encode()
    name = ADMIN_PIPE + (f"-{suffix}" if suffix else "")
    with open(rf"\\.\pipe\{name}", "r+b", buffering=0) as pipe:
        pipe.write(line + b"\n")
        answer = b""
        while not answer.endswith(b"\n"):
            try:
                chunk = pipe.read(1)
            except OSError:
                break
            if not chunk:
                break
            answer += chunk
        return json.loads(answer) if answer else None


def provision_frame(config: dict[str, Any], key: str) -> dict[str, Any]:
    return {"v": 1, "op": "provision", **config, "key": key}


def check_admin_refusals(config: dict[str, Any], suffix: str | None) -> None:
    """Every refused frame gets an answer that says why, and nothing changes."""
    refusals = {
        b"not json": "not_json",
        b'{"v":2,"op":"reload"}': "version",
        b'{"v":1,"op":"format"}': "unknown_operation",
        b'{"v":1,"op":"reload","now":true}': "invalid",
        json.dumps(provision_frame(config, "AAAA")).encode(): "invalid",
        b'{"v":1,"op":"reload","pad":"' + b"x" * 9000 + b'"}': "too_long",
    }
    for frame, code in refusals.items():
        answer = admin_request(frame, suffix)
        assert answer is not None, frame[:40]
        assert answer["ok"] is False and answer["error"] == code, answer


def service_exists() -> bool:
    result = run(["sc.exe", "query", SERVICE], check=False)
    if result.returncode == ERROR_SERVICE_DOES_NOT_EXIST:
        return False
    assert result.returncode == 0, result.stdout
    return True


def pipe_exists(name: str) -> bool:
    try:
        with open(rf"\\.\pipe\{name}", "rb"):
            return True
    except FileNotFoundError:
        return False
    except OSError:
        # Access denied still means that the pipe is there.
        return True


def offer_provisioning(name: str, frame: dict[str, Any]) -> threading.Thread:
    """Offers a provisioning frame on a one-time pipe, as the app will."""
    listener = Listener(rf"\\.\pipe\{name}", family="AF_PIPE")

    def serve() -> None:
        with listener, listener.accept() as connection:
            connection.send_bytes(json.dumps(frame).encode() + b"\n")
            time.sleep(1)

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    return thread


def service_mode(args: argparse.Namespace, binary: Path) -> None:
    assert ctypes.windll.shell32.IsUserAnAdmin(), "--service needs an elevated terminal"
    assert not service_exists(), "the service is already installed on this computer"
    program_data = Path(os.environ["ProgramData"])
    data = program_data / "Deskmate" / "service"
    program = Path(os.environ["ProgramW6432"]) / "Deskmate service"
    work = Path(tempfile.mkdtemp(prefix="deskmate-service-e2e-"))
    key = base64.b64encode(os.urandom(32)).decode()
    key_file = work / "pairing-key"
    key_file.write_text(key, encoding="utf-8")
    hubs: list[Hub] = []
    try:
        hub = start_hub(args, key_file, no_info=False)
        hubs.append(hub)
        config = settings(hub.port)

        step("install")
        refused = run([str(binary), "install"], check=False)
        assert refused.returncode == 2, "a test build is installed only as a dry run"
        print(run([str(binary), "install", "--dry-run-power"]).stdout)
        assert (program / "deskmate-service.exe").is_file()
        assert data.is_dir()
        assert read_status(binary, None)["state"] == "not_provisioned"

        step("the admin pipe refuses what it does not know")
        check_admin_refusals(config, None)
        assert read_status(binary, None)["state"] == "not_provisioned"

        step("provision through the admin pipe")
        assert admin_request(provision_frame(config, key)) == {"v": 1, "ok": True}
        session = check_session_start(hub)
        check_states(hub)
        step("status pipe and fingerprint")
        check_status(binary, None, config, key)
        stored = (data / "config.json").read_text(encoding="utf-8")
        assert key not in stored and "key" not in stored
        step("restart command with the dry-run executor")
        check_restart_command(hub, data / "security.log")
        step("reconnect after the hub closed the session")
        session = check_reconnect(hub, session)
        step("handover to a waiting app and back")
        session = check_handover(hub, session, binary, None)

        step("a stop ends the session; a start finds the keys in the vault")
        run(["sc.exe", "stop", SERVICE])
        hub.closed(session, bye=None)
        hub.nothing(6)
        run(["sc.exe", "start", SERVICE])
        session = check_session_start(hub)

        step("install over the running service, settings through a one-time pipe")
        old_hub = start_hub(args, key_file, no_info=True)
        hubs.append(old_hub)
        pipe_name = f"deskmate-provision-{os.getpid()}"
        offered = offer_provisioning(
            pipe_name, provision_frame(settings(old_hub.port), key)
        )
        update = run(
            [
                str(binary),
                "install",
                "--dry-run-power",
                "--provision-pipe",
                pipe_name,
                "--provision-program",
                Path(sys.executable).name,
            ]
        )
        print(update.stdout)
        offered.join(timeout=10)
        # The stop for the update ended the session without a bye. What the
        # restarted service did at the old address until the new settings
        # arrived is of no interest.
        hub.closed(session, bye=None)
        step("a hub that never answers info")
        check_too_old(old_hub, binary, None)
        for name in ("service.log", "security.log", "config.json"):
            assert key not in (data / name).read_text(encoding="utf-8"), name

        step("uninstall leaves nothing")
        print(run([str(binary), "uninstall"]).stdout)
        check_nothing_left(data, program)

        step("the keys are gone from the vault of the system account")
        hub.drain()
        run([str(binary), "install", "--dry-run-power"])
        (data / "config.json").write_text(json.dumps(config), encoding="utf-8")
        assert admin_request({"v": 1, "op": "reload"}) == {"v": 1, "ok": True}
        status = wait_for_state(binary, None, "error")
        assert status["detail"] == "The pairing key is missing.", status
        hub.nothing(3)
        run([str(binary), "uninstall"])
        check_nothing_left(data, program)
    except BaseException:
        show_tail(data / "service.log")
        raise
    finally:
        for hub in hubs:
            hub.quit()
        if service_exists():
            run([str(binary), "uninstall"], check=False)
        shutil.rmtree(work, ignore_errors=True)


def check_nothing_left(data: Path, program: Path) -> None:
    assert not service_exists(), "the service is still registered"
    assert not data.exists(), f"{data} is still there"
    assert not data.parent.exists(), f"{data.parent} is still there"
    assert not program.exists(), f"{program} is still there"
    assert not pipe_exists(STATUS_PIPE), "the status pipe is still there"
    assert not pipe_exists(ADMIN_PIPE), "the admin pipe is still there"


def step(text: str) -> None:
    print(f"- {text}", flush=True)


def show_tail(path: Path) -> None:
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return
    print(f"--- last lines of {path.name} ---")
    print("\n".join(lines[-40:]))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--console", action="store_true")
    mode.add_argument("--service", action="store_true")
    parser.add_argument(
        "--binary",
        type=Path,
        default=REPO / "src-tauri" / "target" / "debug" / "deskmate-service.exe",
        help="deskmate-service.exe built with --features test-hooks",
    )
    parser.add_argument(
        "--wsl", default="", help="run the fake hub in this WSL distribution"
    )
    parser.add_argument(
        "--wsl-python",
        default="~/deskmate-venvs/test/bin/python",
        help="the Python inside WSL that has aiohttp and cryptography",
    )
    args = parser.parse_args()
    if os.name != "nt":
        print("this test needs Windows")
        return 1
    binary = args.binary.resolve()
    assert binary.is_file(), (
        f"{binary} does not exist; build it with --features test-hooks"
    )
    if args.console:
        console_mode(args, binary)
    else:
        service_mode(args, binary)
    print("deskmate-service end-to-end test passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
