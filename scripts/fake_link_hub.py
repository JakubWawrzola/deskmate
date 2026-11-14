"""A stand-in for Home Assistant's side of Deskmate Link, for tests.

It runs the server side of the v2 handshake with the integration's own crypto
module, so a client that talks to it talks to the reference implementation.
After the handshake it relays: every frame the client sends is printed as one
JSON line on stdout, and every JSON line on stdin is a command from whoever
drives the test.

    python scripts/fake_link_hub.py --key-file <path> [--port 0] [--node my-pc]

stdout, one JSON object per line:
    {"event": "listening", "port": 49152}
    {"event": "rejected", "reason": "auth"}
    {"event": "session", "n": 1, "node": "my-pc"}
    {"event": "frame", "n": 1, "frame": {"t": "declare", ...}}
    {"event": "closed", "n": 1}

stdin, one JSON object per line:
    {"send": {"t": "cmd", "id": 1, "key": "restart", "action": "press"}}
    {"close": true}     closes the current session
    {"quit": true}      stops the hub

The hub answers `ping` and, unless --no-info is given, `info`. It stores
nothing and executes nothing. The pairing key is read from --key-file, or
generated and written there when the file does not exist.
"""

from __future__ import annotations

import argparse
import asyncio
import base64
import importlib.util
import json
import os
import re
import secrets
import sys
import threading
import time
from pathlib import Path
from types import ModuleType
from typing import Any

from aiohttp import WSMsgType, web

INTEGRATION_DIR = (
    Path(__file__).resolve().parent.parent / "custom_components" / "deskmate_link"
)


def _load(name: str) -> ModuleType:
    """Load one module of the integration without importing Home Assistant."""
    spec = importlib.util.spec_from_file_location(
        f"deskmate_link_{name}", INTEGRATION_DIR / f"{name}.py"
    )
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {name}.py from the integration")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


const = _load("const")
crypto = _load("crypto")

NODE_RE = re.compile(const.NODE_ID_PATTERN)


def emit(event: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(event) + "\n")
    sys.stdout.flush()


def read_or_create_key(path: Path) -> str:
    if path.exists():
        return path.read_text(encoding="utf-8").strip()
    key = crypto.gen_psk()
    path.write_text(key, encoding="utf-8")
    if os.name != "nt":
        path.chmod(0o600)
    return key


class FakeHub:
    def __init__(self, args: argparse.Namespace, key: str) -> None:
        self._args = args
        self._key = key
        self._ws: web.WebSocketResponse | None = None
        self._tx: Any = None
        self._sessions = 0
        self.stopped = asyncio.Event()

    def _handshake(self, hello: dict[str, Any]) -> tuple[dict, Any, Any] | str:
        """The welcome frame and both codecs, or the reason for a reject."""
        node = hello.get("node")
        ts = hello.get("ts")
        casc = hello.get("casc")
        client_nonce = crypto.b64d_exact(hello.get("cn"), 16)
        client_public = crypto.b64d_exact(hello.get("epk"), 32)
        if (
            hello.get("t") != "hello"
            or hello.get("v") != const.PROTO_V2
            or not isinstance(node, str)
            or not NODE_RE.fullmatch(node)
            or (self._args.node and node != self._args.node)
            or client_nonce is None
            or client_public is None
            or not isinstance(ts, int)
            or isinstance(ts, bool)
            or casc is not False
        ):
            return const.REJECT_AUTH
        if abs(time.time() - ts) > const.HANDSHAKE_MAX_SKEW_S:
            return const.REJECT_CLOCK
        hello_bytes = crypto.v2_hello_bytes(node, client_nonce, ts, client_public, casc)
        if not crypto.v2_mac_ok(self._key, hello.get("mac"), hello_bytes):
            return const.REJECT_AUTH
        ephemeral = crypto.EphemeralKey()
        shared = ephemeral.exchange(client_public)
        if shared is None:
            return const.REJECT_AUTH
        server_nonce = secrets.token_bytes(16)
        now = int(time.time())
        welcome_bytes = crypto.v2_welcome_bytes(
            hello_bytes, server_nonce, now, ephemeral.public
        )
        key_c2s, key_s2c = crypto.v2_session_keys(
            self._key, shared, hello_bytes, welcome_bytes
        )
        welcome = {
            "t": "welcome",
            "v": const.PROTO_V2,
            "sn": base64.b64encode(server_nonce).decode(),
            "ts": now,
            "epk": base64.b64encode(ephemeral.public).decode(),
            "mac": crypto.v2_mac(self._key, welcome_bytes),
        }
        rx = crypto.FrameCodec(
            key_c2s, const.DIR_C2S, node, "c2s", None, const.PROTO_V2
        )
        tx = crypto.FrameCodec(
            key_s2c, const.DIR_S2C, node, "s2c", None, const.PROTO_V2
        )
        return welcome, rx, tx

    async def handle(self, request: web.Request) -> web.WebSocketResponse:
        ws = web.WebSocketResponse(max_msg_size=4 * 1024 * 1024)
        await ws.prepare(request)
        first = await ws.receive(timeout=10)
        try:
            hello = json.loads(first.data) if first.type == WSMsgType.TEXT else {}
        except ValueError:
            hello = {}
        result = self._handshake(hello if isinstance(hello, dict) else {})
        if isinstance(result, str):
            emit({"event": "rejected", "reason": result})
            await ws.send_str(json.dumps({"t": "reject", "reason": result}))
            await ws.close()
            return ws
        welcome, rx, tx = result
        await ws.send_str(json.dumps(welcome))

        # Like the integration: a new session replaces the one before it.
        previous, self._ws, self._tx = self._ws, ws, tx
        if previous is not None and not previous.closed:
            await previous.close()
        self._sessions += 1
        number = self._sessions
        emit({"event": "session", "n": number, "node": hello["node"]})
        try:
            async for message in ws:
                if message.type != WSMsgType.TEXT:
                    continue
                frame = rx.decrypt(json.loads(message.data))
                emit({"event": "frame", "n": number, "frame": frame})
                await self._auto_answer(ws, tx, frame)
        finally:
            if self._ws is ws:
                self._ws = self._tx = None
            emit({"event": "closed", "n": number})
        return ws

    async def _auto_answer(
        self, ws: web.WebSocketResponse, tx: Any, frame: dict[str, Any]
    ) -> None:
        kind = frame.get("t")
        if kind == "ping":
            answer: dict[str, Any] = {"t": "pong"}
        elif kind == "info" and not self._args.no_info:
            answer = {
                "t": "info",
                "integration": self._args.integration,
                "caps": [cap for cap in self._args.caps.split(",") if cap],
            }
        else:
            return
        await ws.send_str(json.dumps(tx.encrypt(answer)))

    async def run_command(self, command: dict[str, Any]) -> None:
        if command.get("quit"):
            self.stopped.set()
        elif command.get("close"):
            if self._ws is not None:
                await self._ws.close()
        elif isinstance(command.get("send"), dict):
            if self._ws is None or self._tx is None:
                emit({"event": "error", "error": "no session to send to"})
                return
            await self._ws.send_str(json.dumps(self._tx.encrypt(command["send"])))


def pump_stdin(loop: asyncio.AbstractEventLoop, lines: asyncio.Queue[str]) -> None:
    """Runs in a daemon thread, so a blocked read never keeps the hub alive."""
    for line in sys.stdin:
        loop.call_soon_threadsafe(lines.put_nowait, line)
    loop.call_soon_threadsafe(lines.put_nowait, "")


async def read_commands(hub: FakeHub) -> None:
    """Commands arrive on stdin; a closed stdin stops the hub."""
    lines: asyncio.Queue[str] = asyncio.Queue()
    threading.Thread(
        target=pump_stdin, args=(asyncio.get_running_loop(), lines), daemon=True
    ).start()
    while not hub.stopped.is_set():
        line = await lines.get()
        if not line:
            hub.stopped.set()
            return
        try:
            command = json.loads(line)
        except ValueError:
            emit({"event": "error", "error": "a command must be one JSON object"})
            continue
        if isinstance(command, dict):
            await hub.run_command(command)


async def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--key-file", type=Path, required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--node", default="", help="accept only this computer")
    parser.add_argument("--integration", default="0.9.0")
    parser.add_argument("--caps", default="info,scoped_declare,bye")
    parser.add_argument(
        "--no-info",
        action="store_true",
        help="never answer `info`, like an integration older than 0.9.0",
    )
    args = parser.parse_args()

    hub = FakeHub(args, read_or_create_key(args.key_file))
    app = web.Application()
    app.router.add_get(const.WS_URL, hub.handle)
    runner = web.AppRunner(app)
    await runner.setup()
    site = web.TCPSite(runner, args.host, args.port)
    await site.start()
    sockets = site._server.sockets if site._server is not None else []  # noqa: SLF001 - aiohttp has no public way to ask for the port it picked
    emit({"event": "listening", "port": sockets[0].getsockname()[1]})

    reader = asyncio.create_task(read_commands(hub))
    try:
        await hub.stopped.wait()
    finally:
        reader.cancel()
        await runner.cleanup()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(130)
