"""Endpoint WebSocket /api/deskmate_link/ws.

requires_auth=False: uwierzytelnieniem jest handshake HMAC z kluczem parowania
(docs/DESKMATE-LINK.md). Widok nie loguje zadnych sekretow.

Odrzucenie NIE jest juz ciche: klient dostaje ramke `reject` z powodem, a przy
powtarzajacych sie probach nieznanego node'a HA zaklada zgloszenie w Naprawach.
Blokada liczona jest per (node, IP), z osobna zapora dla samego IP - inaczej
jeden zle skonfigurowany komputer blokowal poprawny, gdy oba wychodza tym samym
adresem publicznym (tunel/reverse proxy).
"""
from __future__ import annotations

import json
import logging
import time

from aiohttp import WSMsgType, web

from homeassistant.components.http import HomeAssistantView
from homeassistant.core import HomeAssistant
from homeassistant.helpers import issue_registry as ir

from .const import (
    DOMAIN,
    HANDSHAKE_FAILS_LOCKOUT,
    HANDSHAKE_FAILS_LOCKOUT_IP,
    HANDSHAKE_LOCKOUT_S,
    ISSUE_UNKNOWN_NODE,
    LOG_THROTTLE_S,
    REJECT_AUTH,
    REJECT_LOCKED,
    UNKNOWN_NODE_ISSUE_AFTER,
    WS_URL,
)

_LOGGER = logging.getLogger(__name__)

HELLO_TIMEOUT_S = 15
MAX_TRACKED_KEYS = 512


def safe_node(value: object) -> str:
    """Node z niezaufanej ramki - do logu i tekstu zgloszenia."""
    text = str(value)[:32]
    return "".join(ch for ch in text if ch.isprintable()) or "?"


class DeskmateLinkWsView(HomeAssistantView):
    url = WS_URL
    name = "api:deskmate_link:ws"
    requires_auth = False

    def __init__(self) -> None:
        self._fails: dict[str, list[float]] = {}
        self._logged: dict[str, tuple[float, int]] = {}

    # ── licznik nieudanych prob ──────────────────────────────────

    def _recent(self, key: str) -> list[float]:
        now = time.time()
        fails = [t for t in self._fails.get(key, []) if now - t < HANDSHAKE_LOCKOUT_S]
        if fails:
            self._fails[key] = fails
        else:
            self._fails.pop(key, None)
        return fails

    def _locked_ip(self, ip: str) -> bool:
        return len(self._recent(f"ip:{ip}")) >= HANDSHAKE_FAILS_LOCKOUT_IP

    def _locked_node(self, node: str, ip: str) -> bool:
        return len(self._recent(f"node:{node}@{ip}")) >= HANDSHAKE_FAILS_LOCKOUT

    def _record_fail(self, node: str, ip: str) -> int:
        now = time.time()
        if len(self._fails) > MAX_TRACKED_KEYS:
            # awaryjne czyszczenie - slownik nie moze rosnac w nieskonczonosc
            for key in list(self._fails):
                if not self._recent(key):
                    self._fails.pop(key, None)
        self._fails.setdefault(f"ip:{ip}", []).append(now)
        key = f"node:{node}@{ip}"
        self._fails.setdefault(key, []).append(now)
        return len(self._fails[key])

    # ── log z throttlingiem ──────────────────────────────────────

    def _log_rejected(self, node: str, ip: str, count: int) -> None:
        key = f"{node}@{ip}"
        now = time.time()
        last, suppressed = self._logged.get(key, (0.0, 0))
        if now - last < LOG_THROTTLE_S:
            self._logged[key] = (last, suppressed + 1)
            return
        self._logged[key] = (now, 0)
        _LOGGER.warning(
            "deskmate_link: odrzucony handshake z %s (node=%s, prob w oknie: %s%s)",
            ip,
            node,
            count,
            f", pominietych wpisow: {suppressed}" if suppressed else "",
        )

    # ── zgloszenie w Naprawach ───────────────────────────────────

    def _raise_unknown_node_issue(
        self, hass: HomeAssistant, node: str, ip: str, known: bool
    ) -> None:
        if known:
            return
        ir.async_create_issue(
            hass,
            DOMAIN,
            f"{ISSUE_UNKNOWN_NODE}_{node}",
            is_fixable=False,
            severity=ir.IssueSeverity.WARNING,
            translation_key=ISSUE_UNKNOWN_NODE,
            translation_placeholders={"node": node, "ip": ip},
        )

    async def _reject(
        self, ws: web.WebSocketResponse, reason: str
    ) -> web.StreamResponse:
        try:
            await ws.send_str(json.dumps({"t": "reject", "reason": reason}))
        except (ConnectionResetError, RuntimeError):  # klient juz sie rozlaczyl
            pass
        await ws.close()
        return ws

    async def get(self, request: web.Request) -> web.StreamResponse:
        hass: HomeAssistant = request.app["hass"]
        ip = request.remote or "?"
        if self._locked_ip(ip):
            return web.Response(status=429, text="locked out")

        ws = web.WebSocketResponse(heartbeat=30)
        await ws.prepare(request)

        # 1. Czekaj na hello
        try:
            msg = await ws.receive(timeout=HELLO_TIMEOUT_S)
        except TimeoutError:
            await ws.close()
            return ws
        if msg.type != WSMsgType.TEXT:
            await ws.close()
            return ws
        try:
            hello = json.loads(msg.data)
        except json.JSONDecodeError:
            self._record_fail("?", ip)
            return await self._reject(ws, REJECT_AUTH)
        if not isinstance(hello, dict) or hello.get("t") != "hello":
            self._record_fail("?", ip)
            return await self._reject(ws, REJECT_AUTH)

        node = safe_node(hello.get("node", "?"))
        if self._locked_node(node, ip):
            return await self._reject(ws, REJECT_LOCKED)

        # 2. Znajdz hub, ktorego klucz potwierdza MAC (nieprzypiety wpis
        #    przyjmuje dowolny node - to jest wlasnie parowanie)
        hubs = hass.data.get(DOMAIN, {}).get("hubs", {})
        result = None
        matched = None
        for hub in hubs.values():
            result = hub.try_handshake(hello)
            if result is not None:
                matched = hub
                break
        if matched is None or result is None:
            count = self._record_fail(node, ip)
            self._log_rejected(node, ip, count)
            if count >= UNKNOWN_NODE_ISSUE_AFTER:
                known = any(h.node_id == node for h in hubs.values())
                self._raise_unknown_node_issue(hass, node, ip, known)
            return await self._reject(ws, REJECT_AUTH)

        welcome, rx, tx, claimed_node = result
        await matched.async_claim(claimed_node)

        # 3. Welcome + petla sesji w hubie
        self._fails.pop(f"node:{claimed_node}@{ip}", None)
        ir.async_delete_issue(hass, DOMAIN, f"{ISSUE_UNKNOWN_NODE}_{claimed_node}")
        await ws.send_str(json.dumps(welcome))
        _LOGGER.info("deskmate_link[%s]: polaczono (%s)", matched.node_id, ip)
        await matched.run_session(ws, rx, tx)
        _LOGGER.info("deskmate_link[%s]: rozlaczono", matched.node_id)
        return ws
