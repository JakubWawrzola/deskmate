"""End-to-end test of the Linux daemon against the fake hub.

Not a pytest module: it starts real processes. Run it on Linux by hand or
from CI:

    python3 tests/daemon_e2e.py [--binary <deskmate-service>] [--hub-python <python>]

It pairs the daemon into temporary folders with `pair`, runs it in the
foreground with `console`, and drives it with signals and the fake hub. It
touches nothing outside its temporary folder, needs no root, and installs
nothing. The binary must be built with `--features test-hooks`: power
commands are then recorded, never run. The Python that runs the fake hub
needs aiohttp and cryptography; this script needs only the standard library.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import json
import os
import queue
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import IO, Any

REPO = Path(__file__).resolve().parent.parent
NODE = "test_pc"
WAIT_S = 45
PUBLISH_INTERVAL_S = 2
# The daemon refuses power commands this long after boot or a wake-up.
QUIET_AFTER_START_S = 300
# Longer than the 10 s the daemon waits for the hub's `info`.
NO_INFO_WAIT_S = 25
PAIRING_KEY_FILE = "machine.deskmate-service-link.key"
REFUSAL = "not supported by the Deskmate daemon"


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


def start_hub(python: str, key_file: Path, *, no_info: bool) -> Hub:
    command = [
        python,
        str(REPO / "scripts" / "fake_link_hub.py"),
        "--key-file",
        str(key_file),
        "--port",
        "0",
        "--node",
        NODE,
    ]
    if no_info:
        command.append("--no-info")
    return Hub(command)


def pairing_code(key: str, port: int) -> str:
    """A code as the integration makes it: base64url of JSON, no padding."""
    data = {"k": key, "u": f"ws://127.0.0.1:{port}", "n": NODE}
    body = base64.urlsafe_b64encode(json.dumps(data).encode()).decode()
    return "DMP1." + body.rstrip("=")


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


class Folders:
    """The temporary folders a run uses in place of the real ones."""

    def __init__(self, work: Path) -> None:
        self.config = work / "etc"
        self.state = work / "state"
        self.status = work / "run" / "status.json"
        self.status.parent.mkdir()

    def options(self) -> list[str]:
        return ["--config-dir", str(self.config), "--data-dir", str(self.state)]

    def status_options(self) -> list[str]:
        return ["--status-file", str(self.status)]


def run(
    command: list[str], *, stdin: str = "", check: bool = True
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(  # noqa: S603 - the daemon binary under test, with arguments built here
        command, input=stdin, capture_output=True, text=True, check=False, timeout=60
    )
    if check and result.returncode != 0:
        raise AssertionError(
            f"{command} exited with {result.returncode}\n{result.stdout}\n{result.stderr}"
        )
    return result


def mode(path: Path) -> int:
    return stat.S_IMODE(path.stat().st_mode)


def check_pairing(binary: Path, folders: Folders, key: str, port: int) -> None:
    """`pair` refuses a code on the command line and text that is no code,
    then writes private files the daemon can use."""
    code = pairing_code(key, port)
    refused = run([str(binary), "pair", *folders.options(), code], check=False)
    assert refused.returncode == 2, refused
    for text in ["", "not a code\n", key + "\n", "DMP1.!!!!\n"]:
        refused = run(
            [str(binary), "pair", *folders.options()], stdin=text, check=False
        )
        assert refused.returncode == 1, refused
        assert key not in refused.stdout + refused.stderr
        assert not folders.config.exists(), "a refused code writes nothing"
    paired = run([str(binary), "pair", *folders.options()], stdin=code + "\n")
    assert f"Paired as {NODE}" in paired.stdout, paired.stdout
    assert key not in paired.stdout + paired.stderr

    settings = folders.config / "config.json"
    key_file = folders.state / PAIRING_KEY_FILE
    assert mode(settings) == 0o600, oct(mode(settings))
    assert mode(key_file) == 0o600, oct(mode(key_file))
    assert mode(folders.state) == 0o700, oct(mode(folders.state))
    assert key_file.read_text(encoding="utf-8") == key
    stored = settings.read_text(encoding="utf-8")
    assert key not in stored and "key" not in stored


def configure(folders: Folders, link_url: str) -> dict[str, Any]:
    """What a person may change in config.json after pairing."""
    settings = folders.config / "config.json"
    config = json.loads(settings.read_text(encoding="utf-8"))
    config.update(
        link_url=link_url,
        publish_interval_secs=PUBLISH_INTERVAL_S,
        device_name="E2E test PC",
        sensors_enabled={"battery": False},
    )
    settings.write_text(json.dumps(config), encoding="utf-8")
    return config


def hub_url(port: int) -> str:
    return f"ws://127.0.0.1:{port}/api/deskmate_link/ws"


def check_session_start(hub: Hub) -> int:
    """A session opens: info first, then the machine declare, then states."""
    session = hub.session()
    info = hub.frame()
    assert info["t"] == "info", info
    assert info["role"] == "daemon", info
    assert info["platform"] == "linux", info
    # The hub answers info by itself; nothing else may come before declare.
    declare = hub.frame()
    assert declare["t"] == "declare", declare
    assert declare["scope"] == "machine", declare
    assert declare["device"]["name"] == "E2E test PC", declare
    assert declare["device"]["model"] == "Deskmate for Linux", declare
    keys = {entity["key"] for entity in declare["entities"]}
    assert {"cpu", "memory", "uptime"} <= keys, keys
    assert {"sleep", "shutdown", "restart"} <= keys, keys
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


def read_status(binary: Path, folders: Folders) -> dict[str, Any]:
    output = run([str(binary), "status", *folders.status_options()]).stdout
    status = json.loads(output)
    assert set(status) == {"v", "version", "state", "node_id", "detail", "config"}
    assert status["v"] == 1, status
    return status


def wait_for_state(binary: Path, folders: Folders, state: str) -> dict[str, Any]:
    deadline = time.monotonic() + WAIT_S
    while True:
        try:
            status = read_status(binary, folders)
        except AssertionError:
            status = {}
        if status.get("state") == state:
            return status
        assert time.monotonic() < deadline, (
            f"the state stayed {status}, expected {state}"
        )
        time.sleep(0.3)


def check_status(
    binary: Path, folders: Folders, config: dict[str, Any], key: str
) -> None:
    status = wait_for_state(binary, folders, "connected")
    assert status["node_id"] == NODE, status
    assert status["config"] == fingerprint(config, key), "the fingerprint differs"
    text = folders.status.read_text(encoding="utf-8")
    assert key not in text
    assert config["link_url"] not in text, "no address in the status"
    assert text.count("\n") == 1, "one JSON object on one line"
    assert mode(folders.status) == 0o644, oct(mode(folders.status))
    assert not folders.status.with_name("status.json.tmp").exists()


def uptime_s() -> float:
    return float(Path("/proc/uptime").read_text(encoding="ascii").split()[0])


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
    assert ack == {"t": "ack", "id": 43, "ok": False, "error": REFUSAL}, ack
    time.sleep(1)
    assert abs((time.time() - uptime_s()) - boot_time) < 5, "the computer restarted"


def check_refusals(hub: Hub) -> None:
    """Requests that need a user's session are answered at once with a no."""
    hub.send({"t": "notify", "id": 51, "title": "Hello", "message": "there"})
    ack = hub.frame_of("ack")
    assert ack == {"t": "ack", "id": 51, "ok": False, "error": REFUSAL}, ack
    hub.send({"t": "fs", "id": 52, "op": "roots"})
    answer = hub.frame_of("fs_res")
    assert answer == {"t": "fs_res", "id": 52, "ok": False, "error": REFUSAL}, answer


def check_too_old(hub: Hub, binary: Path, folders: Folders) -> None:
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
    status = wait_for_state(binary, folders, "integration_too_old")
    assert status["node_id"] == NODE, status
    assert "daemon" in status["detail"], status
    hub.nothing(5)


def daemon_mode(args: argparse.Namespace, binary: Path) -> None:
    work = Path(tempfile.mkdtemp(prefix="deskmate-daemon-e2e-"))
    folders = Folders(work)
    key = base64.b64encode(os.urandom(32)).decode()
    hub_key = work / "hub-key"
    hub_key.write_text(key, encoding="utf-8")
    hub_key.chmod(0o600)
    hubs: list[Hub] = []
    daemon: subprocess.Popen[str] | None = None
    log: IO[str] | None = None
    try:
        hub = start_hub(args.hub_python, hub_key, no_info=False)
        hubs.append(hub)

        step("pair: refusals, then the files")
        check_pairing(binary, folders, key, hub.port)
        config = configure(folders, hub_url(hub.port))

        step("status without a running daemon")
        missing = run([str(binary), "status", *folders.status_options()], check=False)
        assert missing.returncode == 1 and "not running" in missing.stderr, missing

        log = open(work / "stderr.log", "w", encoding="utf-8")
        daemon = subprocess.Popen(  # noqa: S603 - the daemon binary under test, with arguments built here
            [
                str(binary),
                "console",
                *folders.options(),
                *folders.status_options(),
                "--dry-run-power",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
        )
        assert daemon.stdin is not None and daemon.stdout is not None
        assert daemon.stdout.readline().strip() == "ready"

        def deliver(number: signal.Signals) -> None:
            assert daemon is not None
            os.kill(daemon.pid, number)

        step("session, info with the role daemon, machine declare, states")
        session = check_session_start(hub)
        check_states(hub)
        step("status file and fingerprint")
        check_status(binary, folders, config, key)
        step("notify and fs are refused at once")
        check_refusals(hub)
        step("restart command with the dry-run executor")
        check_restart_command(hub, folders.state / "security.log")

        step("reconnect after the hub closed the session")
        hub.command({"close": True})
        hub.closed(session, bye=None)
        session = check_session_start(hub)

        step("SIGUSR1 before sleep, SIGUSR2 after it")
        deliver(signal.SIGUSR1)
        hub.closed(session, bye="suspend")
        wait_for_state(binary, folders, "waiting")
        hub.nothing(4)
        deliver(signal.SIGUSR2)
        session = check_session_start(hub)

        step("SIGHUP reloads: a hub that never answers info")
        old_hub = start_hub(args.hub_python, hub_key, no_info=True)
        hubs.append(old_hub)
        configure(folders, hub_url(old_hub.port))
        deliver(signal.SIGHUP)
        hub.closed(session, bye=None)
        check_too_old(old_hub, binary, folders)

        step("SIGHUP brings the first hub back")
        configure(folders, hub_url(hub.port))
        deliver(signal.SIGHUP)
        session = check_session_start(hub)

        step("SIGTERM ends the session and the daemon")
        deliver(signal.SIGTERM)
        hub.closed(session, bye=None)
        assert daemon.wait(timeout=WAIT_S) == 0
        assert not folders.status.exists(), "the status file is removed"
        log.flush()
        logged = (work / "stderr.log").read_text(encoding="utf-8")
        assert "<6>Deskmate daemon" in logged, "journal priorities"
        for name, text in [
            ("stderr", logged),
            ("security.log", (folders.state / "security.log").read_text("utf-8")),
            ("config.json", (folders.config / "config.json").read_text("utf-8")),
        ]:
            assert key not in text, name

        step("a second run, ended by a shutdown")
        hub_shutdown(binary, folders, hub)

        step("unpair removes the settings and the key")
        run([str(binary), "unpair", *folders.options()])
        assert not (folders.config / "config.json").exists()
        assert not (folders.state / PAIRING_KEY_FILE).exists()
    except BaseException:
        show_tail(work / "stderr.log")
        raise
    finally:
        if daemon is not None and daemon.poll() is None:
            daemon.kill()
            daemon.wait(timeout=10)
        if log is not None:
            log.close()
        for hub in hubs:
            hub.quit()
        shutil.rmtree(work, ignore_errors=True)


def hub_shutdown(binary: Path, folders: Folders, hub: Hub) -> None:
    """A shutdown says bye with the reason shutdown and ends the run."""
    daemon = subprocess.Popen(  # noqa: S603 - the daemon binary under test, with arguments built here
        [
            str(binary),
            "console",
            *folders.options(),
            *folders.status_options(),
            "--dry-run-power",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        assert daemon.stdin is not None and daemon.stdout is not None
        assert daemon.stdout.readline().strip() == "ready"
        session = check_session_start(hub)
        daemon.stdin.write("shutdown\n")
        daemon.stdin.flush()
        hub.closed(session, bye="shutdown")
        assert daemon.wait(timeout=WAIT_S) == 0
    finally:
        if daemon.poll() is None:
            daemon.kill()
            daemon.wait(timeout=10)


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
    parser.add_argument(
        "--binary",
        type=Path,
        default=REPO / "src-tauri" / "target" / "debug" / "deskmate-service",
        help="deskmate-service built for Linux with --features test-hooks",
    )
    parser.add_argument(
        "--hub-python",
        default=sys.executable,
        help="the Python that runs the fake hub; it needs aiohttp and cryptography",
    )
    args = parser.parse_args()
    if not sys.platform.startswith("linux"):
        print("this test needs Linux")
        return 1
    binary = args.binary.resolve()
    assert binary.is_file(), (
        f"{binary} does not exist; build it with --features test-hooks"
    )
    daemon_mode(args, binary)
    print("deskmate daemon end-to-end test passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
