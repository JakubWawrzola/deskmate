"""A Link client for the tests, built on the integration's own crypto module.

It plays the computer's side of the protocol: the v2 handshake from
docs/LINK.md and encrypted session frames. Every key it uses is generated
inside the test that uses it.
"""

from __future__ import annotations

import base64
import json
import secrets
import time
from collections.abc import Awaitable, Callable
from typing import Any

from aiohttp import ClientWebSocketResponse, WSMsgType
from aiohttp.test_utils import TestClient

from custom_components.deskmate_link.const import (
    DIR_C2S,
    DIR_S2C,
    PROTO_V2,
    WS_URL,
)
from custom_components.deskmate_link.crypto import (
    EphemeralKey,
    FrameCodec,
    b64d_exact,
    v2_hello_bytes,
    v2_mac,
    v2_mac_ok,
    v2_session_keys,
    v2_welcome_bytes,
)

# The computer that the tests connect as.
NODE_ID = "test_pc"
RECEIVE_TIMEOUT_S = 5
_CLOSE_TYPES = (WSMsgType.CLOSE, WSMsgType.CLOSING, WSMsgType.CLOSED)


class LinkRejectedError(Exception):
    """Home Assistant answered the handshake with a `reject` frame."""

    def __init__(self, reason: str) -> None:
        super().__init__(reason)
        self.reason = reason


class LinkClient:
    """One authenticated Link session, seen from the computer."""

    def __init__(
        self,
        ws: ClientWebSocketResponse,
        welcome: dict[str, Any],
        tx: FrameCodec,
        rx: FrameCodec,
    ) -> None:
        self._ws = ws
        self.welcome = welcome
        self._tx = tx
        self._rx = rx

    @classmethod
    async def connect(cls, http: TestClient, node: str, key: str) -> LinkClient:
        """Run the v2 handshake. Raises LinkRejectedError when it is refused."""
        ws = await http.ws_connect(WS_URL)
        ephemeral = EphemeralKey()
        client_nonce = secrets.token_bytes(16)
        timestamp = int(time.time())
        hello_bytes = v2_hello_bytes(
            node, client_nonce, timestamp, ephemeral.public, False
        )
        await ws.send_str(
            json.dumps(
                {
                    "t": "hello",
                    "v": PROTO_V2,
                    "node": node,
                    "cn": base64.b64encode(client_nonce).decode(),
                    "ts": timestamp,
                    "epk": base64.b64encode(ephemeral.public).decode(),
                    "casc": False,
                    "mac": v2_mac(key, hello_bytes),
                }
            )
        )
        answer = json.loads((await ws.receive(timeout=RECEIVE_TIMEOUT_S)).data)
        if answer["t"] == "reject":
            await ws.close()
            raise LinkRejectedError(answer["reason"])

        assert answer["t"] == "welcome"
        server_nonce = b64d_exact(answer["sn"], 16)
        server_public = b64d_exact(answer["epk"], 32)
        assert server_nonce is not None
        assert server_public is not None
        welcome_bytes = v2_welcome_bytes(
            hello_bytes, server_nonce, answer["ts"], server_public
        )
        assert v2_mac_ok(key, answer["mac"], welcome_bytes)
        shared = ephemeral.exchange(server_public)
        assert shared is not None
        key_c2s, key_s2c = v2_session_keys(key, shared, hello_bytes, welcome_bytes)
        return cls(
            ws,
            answer,
            FrameCodec(key_c2s, DIR_C2S, node, "c2s", None, PROTO_V2),
            FrameCodec(key_s2c, DIR_S2C, node, "s2c", None, PROTO_V2),
        )

    async def send(self, payload: dict[str, Any]) -> None:
        """Encrypt one session frame and send it."""
        await self._ws.send_str(json.dumps(self._tx.encrypt(payload)))

    async def receive(self) -> dict[str, Any]:
        """Wait for the next session frame and decrypt it."""
        message = await self._ws.receive(timeout=RECEIVE_TIMEOUT_S)
        assert message.type == WSMsgType.TEXT
        return self._rx.decrypt(json.loads(message.data))

    async def sync(self) -> None:
        """Return once the hub has handled every frame sent so far.

        The hub handles frames in order and answers `ping` with `pong`, so
        the `pong` proves that everything before the `ping` was processed. It
        also proves that the hub sent nothing else in the meantime.
        """
        await self.send({"t": "ping"})
        assert await self.receive() == {"t": "pong"}

    async def wait_closed(self) -> None:
        """Wait until Home Assistant closes this session."""
        message = await self._ws.receive(timeout=RECEIVE_TIMEOUT_S)
        assert message.type in _CLOSE_TYPES

    async def close(self) -> None:
        await self._ws.close()


# The `connect` fixture: opens a session, with the entry's key by default.
type Connect = Callable[..., Awaitable[LinkClient]]
