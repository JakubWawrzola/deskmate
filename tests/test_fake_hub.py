"""The fake hub in scripts/ speaks Link like the integration does.

The Windows service and the Linux daemon are tested against that script, so
these tests hold it to the same client the integration tests use.
"""

from __future__ import annotations

import asyncio
import json
import sys
from collections.abc import AsyncGenerator
from pathlib import Path
from typing import Any

import pytest
from aiohttp import ClientSession

from custom_components.deskmate_link.crypto import gen_psk

from .link_client import NODE_ID, LinkClient, LinkRejectedError

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "fake_link_hub.py"
WAIT_TIMEOUT_S = 10


class Hub:
    """The fake hub as a child process, driven through its stdin and stdout."""

    def __init__(
        self, process: asyncio.subprocess.Process, key: str, port: int
    ) -> None:
        self._process = process
        self.key = key
        self.port = port

    async def event(self) -> dict[str, Any]:
        assert self._process.stdout is not None
        async with asyncio.timeout(WAIT_TIMEOUT_S):
            return json.loads(await self._process.stdout.readline())

    async def command(self, command: dict[str, Any]) -> None:
        assert self._process.stdin is not None
        self._process.stdin.write((json.dumps(command) + "\n").encode())
        await self._process.stdin.drain()

    async def quit(self) -> int:
        await self.command({"quit": True})
        async with asyncio.timeout(WAIT_TIMEOUT_S):
            return await self._process.wait()


@pytest.fixture
async def hub(tmp_path: Path, socket_enabled: None) -> AsyncGenerator[Hub]:
    key_file = tmp_path / "pairing.key"
    process = await asyncio.create_subprocess_exec(
        sys.executable,
        str(SCRIPT),
        "--key-file",
        str(key_file),
        "--node",
        NODE_ID,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
    )
    assert process.stdout is not None
    async with asyncio.timeout(WAIT_TIMEOUT_S):
        listening = json.loads(await process.stdout.readline())
    assert listening["event"] == "listening"
    yield Hub(process, key_file.read_text(encoding="utf-8"), listening["port"])
    if process.returncode is None:
        process.kill()
        await process.wait()


@pytest.fixture
async def http(hub: Hub) -> AsyncGenerator[ClientSession]:
    async with ClientSession(base_url=f"http://127.0.0.1:{hub.port}") as session:
        yield session


async def test_frames_travel_both_ways(hub: Hub, http: ClientSession) -> None:
    client = await LinkClient.connect(http, NODE_ID, hub.key)
    assert await hub.event() == {"event": "session", "n": 1, "node": NODE_ID}

    info = {"t": "info", "role": "service", "platform": "windows", "app": "0.9.0"}
    await client.send(info)
    assert await hub.event() == {"event": "frame", "n": 1, "frame": info}
    answer = await client.receive()
    assert answer["t"] == "info"
    assert "scoped_declare" in answer["caps"]

    command = {"t": "cmd", "id": 1, "key": "restart", "action": "press"}
    await hub.command({"send": command})
    assert await client.receive() == command

    await client.sync()
    assert (await hub.event())["frame"] == {"t": "ping"}

    await client.close()
    assert await hub.event() == {"event": "closed", "n": 1}
    assert await hub.quit() == 0


async def test_wrong_key_is_rejected(hub: Hub, http: ClientSession) -> None:
    with pytest.raises(LinkRejectedError) as rejected:
        await LinkClient.connect(http, NODE_ID, gen_psk())
    assert rejected.value.reason == "auth"
    assert await hub.event() == {"event": "rejected", "reason": "auth"}
    assert await hub.quit() == 0


async def test_another_computer_is_rejected(hub: Hub, http: ClientSession) -> None:
    with pytest.raises(LinkRejectedError):
        await LinkClient.connect(http, "other_pc", hub.key)
    assert await hub.quit() == 0
