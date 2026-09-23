"""Hub sesji Deskmate Link - jedno urzadzenie (node) per config entry."""
from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import Any

from aiohttp import WSMsgType, web

from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers.dispatcher import async_dispatcher_send
from homeassistant.helpers.storage import Store

from .const import (
    CONF_CASCADE_KEY,
    CONF_KEY,
    CONF_NODE_ID,
    DIR_C2S,
    DIR_S2C,
    EVENT_NOTIFY_ACTION,
    EVENT_TRIGGER,
    HANDSHAKE_MAX_SKEW_S,
    PERSIST_DELAY_S,
    PROTO_VERSION,
    SIGNAL_AVAILABLE,
    SIGNAL_DECLARED,
    SIGNAL_STATE,
    SIGNAL_TRIGGER,
    STORAGE_VERSION,
)
from .crypto import (
    FrameCodec,
    derive_cascade_keys,
    derive_session_keys,
    gen_nonce16,
    hs_mac,
    hs_mac_ok,
)

_LOGGER = logging.getLogger(__name__)

CMD_ACK_TIMEOUT = 10.0
FS_RESPONSE_TIMEOUT = 15.0

# Wynik udanego handshake'u: (welcome, kodek c2s, kodek s2c, node)
HandshakeResult = tuple[dict, FrameCodec, FrameCodec, str]


class DeskmateHub:
    """Stan jednego node'a: sesja WS, encje, stany."""

    def __init__(self, hass: HomeAssistant, entry) -> None:
        self.hass = hass
        self.entry = entry
        # Pusty node_id = wpis jeszcze nieprzypiety; przypnie sie do pierwszego
        # klienta, ktory poprawnie uwierzytelni sie kluczem parowania.
        self.node_id: str = entry.data.get(CONF_NODE_ID) or ""
        self._psk: str = entry.data[CONF_KEY]
        self._cascade_key: str = entry.data.get(CONF_CASCADE_KEY) or ""

        self.device_info: dict[str, Any] = {}
        self.entities: list[dict[str, Any]] = []  # deskryptory z declare
        self.states: dict[str, Any] = {}
        self.available: bool = False

        self._ws: web.WebSocketResponse | None = None
        self._tx: FrameCodec | None = None  # s2c
        self._rx: FrameCodec | None = None  # c2s
        self._send_lock = asyncio.Lock()
        self._cmd_id = 0
        self._pending: dict[int, asyncio.Future] = {}
        self._seen_hello: dict[str, float] = {}
        self._store: Store = Store(
            hass, STORAGE_VERSION, f"{self.entry.domain}.{entry.entry_id}"
        )

    # ── trwalosc deklaracji encji ────────────────────────────────

    async def async_restore(self) -> None:
        """Odtwarza ostatni `declare`, zeby encje istnialy zaraz po restarcie HA.

        Odpowiednik retained discovery w MQTT: bez tego encje pojawiaja sie
        dopiero po polaczeniu komputera i do tego czasu dashboardy pokazuja
        wylacznie duchy z rejestru.
        """
        data = await self._store.async_load()
        if not data:
            return
        self.device_info = data.get("device", {}) or {}
        self.entities = list(data.get("entities", []) or [])

    def _persist(self) -> None:
        self._store.async_delay_save(
            lambda: {"device": self.device_info, "entities": self.entities},
            PERSIST_DELAY_S,
        )

    async def async_remove_storage(self) -> None:
        await self._store.async_remove()

    # ── handshake ────────────────────────────────────────────────

    def _accept_hello_nonce(self, cn: str, ts: float) -> bool:
        """Odrzuca powtorzone `hello` (ta sama cn) w oknie tolerancji zegara.

        Bez tego podsluchana ramka hello, odtworzona w ciagu 90 s, wywracala
        zywa sesje - MAC sie zgadza, wiec serwer wital "klienta" i zamykal
        poprzednie polaczenie.
        """
        now = time.time()
        cutoff = HANDSHAKE_MAX_SKEW_S * 2
        self._seen_hello = {
            nonce: seen for nonce, seen in self._seen_hello.items() if now - seen < cutoff
        }
        if not cn or cn in self._seen_hello:
            return False
        self._seen_hello[cn] = now
        return True

    def try_handshake(self, hello: dict) -> HandshakeResult | None:
        """Weryfikuje hello; zwraca (welcome, rx, tx, node) albo None.

        NIE zmienia stanu huba - kodeki sesji instaluje dopiero `run_session`.
        """
        node = hello.get("node")
        if not isinstance(node, str) or not node:
            return None
        if self.node_id and node != self.node_id:
            return None
        if hello.get("v") != PROTO_VERSION:
            return None
        cn = hello.get("cn", "")
        ts = hello.get("ts", 0)
        if not isinstance(ts, (int, float)) or abs(time.time() - ts) > HANDSHAKE_MAX_SKEW_S:
            return None
        if not hs_mac_ok(self._psk, hello.get("mac", ""), "hello", node, cn, str(int(ts))):
            return None
        if not self._accept_hello_nonce(cn, ts):
            return None
        # Kaskada musi byc wlaczona po obu stronach albo po zadnej. Cicha zgoda
        # na slabszy wariant byla by najgorszym mozliwym zachowaniem.
        if bool(hello.get("casc")) != bool(self._cascade_key):
            _LOGGER.warning(
                "deskmate_link[%s]: niezgodna kaskada (klient=%s, HA=%s)",
                node,
                bool(hello.get("casc")),
                bool(self._cascade_key),
            )
            return None
        sn = gen_nonce16()
        now = int(time.time())
        k_c2s, k_s2c = derive_session_keys(self._psk, cn, sn)
        casc_c2s = casc_s2c = None
        if self._cascade_key:
            casc_c2s, casc_s2c = derive_cascade_keys(self._cascade_key, cn, sn)
        rx = FrameCodec(k_c2s, DIR_C2S, node, "c2s", casc_c2s)
        tx = FrameCodec(k_s2c, DIR_S2C, node, "s2c", casc_s2c)
        welcome = {
            "t": "welcome",
            "sn": sn,
            "ts": now,
            "mac": hs_mac(self._psk, "welcome", node, cn, sn, str(now)),
        }
        return welcome, rx, tx, node

    async def async_claim(self, node_id: str) -> None:
        """Przypina nieprzypiety wpis do node'a, ktory sie uwierzytelnil."""
        if self.node_id == node_id:
            return
        self.node_id = node_id
        self.hass.config_entries.async_update_entry(
            self.entry,
            title=node_id,
            unique_id=node_id,
            data={**self.entry.data, CONF_NODE_ID: node_id},
        )
        _LOGGER.info("deskmate_link: wpis przypiety do node'a %s", node_id)

    # ── petla polaczenia (wolane z widoku WS) ────────────────────

    async def run_session(
        self, ws: web.WebSocketResponse, rx: FrameCodec, tx: FrameCodec
    ) -> None:
        if self._ws is not None:
            _LOGGER.info("deskmate_link[%s]: nowa sesja zastepuje stara", self.node_id)
            await self._close_current()
        self._ws = ws
        self._rx = rx
        self._tx = tx
        self._set_available(True)
        try:
            async for msg in ws:
                if msg.type != WSMsgType.TEXT:
                    break
                try:
                    frame = json.loads(msg.data)
                    if frame.get("t") != "e":
                        continue
                    payload = self._rx.decrypt(frame)  # type: ignore[union-attr]
                except (ValueError, KeyError, json.JSONDecodeError) as err:
                    _LOGGER.warning(
                        "deskmate_link[%s]: uszkodzona/replay ramka (%s) - rozlaczam",
                        self.node_id,
                        err,
                    )
                    break
                await self._handle(payload)
        finally:
            if self._ws is ws:
                self._ws = None
                self._tx = None
                self._rx = None
                self._set_available(False)
                self._fail_pending("disconnected")

    async def _close_current(self) -> None:
        ws, self._ws = self._ws, None
        if ws is not None and not ws.closed:
            try:
                await ws.close()
            except Exception:  # noqa: BLE001 - zamkniecie best-effort
                pass

    def _set_available(self, value: bool) -> None:
        self.available = value
        async_dispatcher_send(self.hass, f"{SIGNAL_AVAILABLE}_{self.entry.entry_id}")

    def _fail_pending(self, reason: str) -> None:
        for fut in self._pending.values():
            if not fut.done():
                fut.set_exception(ConnectionError(reason))
        self._pending.clear()

    # ── wiadomosci od klienta ────────────────────────────────────

    async def _handle(self, msg: dict) -> None:
        mtype = msg.get("t")
        if mtype == "declare":
            self.device_info = msg.get("device", {}) or {}
            self.entities = list(msg.get("entities", []) or [])
            self._prune_stale_registry_entries()
            self._persist()
            async_dispatcher_send(self.hass, f"{SIGNAL_DECLARED}_{self.entry.entry_id}")
        elif mtype == "state":
            updates = msg.get("s", {}) or {}
            if isinstance(updates, dict):
                self.states.update(updates)
                async_dispatcher_send(self.hass, f"{SIGNAL_STATE}_{self.entry.entry_id}")
        elif mtype == "ack":
            fut = self._pending.pop(msg.get("id"), None)
            if fut is not None and not fut.done():
                if msg.get("ok"):
                    fut.set_result(True)
                else:
                    fut.set_exception(RuntimeError(str(msg.get("error", "error"))))
        elif mtype == "fs_res":
            fut = self._pending.pop(msg.get("id"), None)
            if fut is not None and not fut.done():
                fut.set_result(msg)
        elif mtype == "notify_action":
            self.hass.bus.async_fire(
                EVENT_NOTIFY_ACTION,
                {"node_id": self.node_id, "action": msg.get("action", "")},
            )
        elif mtype == "trigger":
            # Hotkey/zdarzenie z klienta: encja event + event na busie HA.
            key = str(msg.get("key", ""))
            event_type = str(msg.get("event", "press"))
            if key:
                async_dispatcher_send(
                    self.hass,
                    f"{SIGNAL_TRIGGER}_{self.entry.entry_id}_{key}",
                    event_type,
                )
                self.hass.bus.async_fire(
                    EVENT_TRIGGER,
                    {"node_id": self.node_id, "key": key, "event": event_type},
                )
        elif mtype == "ping":
            await self._send({"t": "pong"})
        # pong: nic do zrobienia

    def _prune_stale_registry_entries(self) -> None:
        """Usuwa z rejestru encje, ktorych nie ma w nowym declare."""
        from homeassistant.helpers import entity_registry as er

        declared = {f"{self.entry.entry_id}-{d['key']}" for d in self.entities if d.get("key")}
        registry = er.async_get(self.hass)
        for entry in er.async_entries_for_config_entry(registry, self.entry.entry_id):
            if entry.unique_id not in declared:
                _LOGGER.info(
                    "deskmate_link[%s]: usuwam nieaktualna encje %s",
                    self.node_id,
                    entry.entity_id,
                )
                registry.async_remove(entry.entity_id)

    # ── wysylka do klienta ───────────────────────────────────────

    async def _send(self, payload: dict) -> None:
        async with self._send_lock:
            ws, tx = self._ws, self._tx
            if ws is None or tx is None:
                raise ConnectionError("deskmate not connected")
            await ws.send_str(json.dumps(tx.encrypt(payload)))

    async def _send_with_ack(self, payload: dict) -> None:
        self._cmd_id += 1
        cmd_id = self._cmd_id
        payload["id"] = cmd_id
        fut: asyncio.Future = self.hass.loop.create_future()
        self._pending[cmd_id] = fut
        try:
            await self._send(payload)
            async with asyncio.timeout(CMD_ACK_TIMEOUT):
                await fut
        finally:
            self._pending.pop(cmd_id, None)

    async def async_command(self, key: str, action: str, value: Any = None) -> None:
        payload: dict[str, Any] = {"t": "cmd", "key": key, "action": action}
        if value is not None:
            payload["value"] = value
        await self._send_with_ack(payload)

    async def async_notify(
        self,
        title: str,
        message: str,
        image: str | None = None,
        actions: list[dict] | None = None,
    ) -> None:
        payload: dict[str, Any] = {"t": "notify", "title": title, "message": message}
        if image:
            payload["image"] = image
        if actions:
            payload["actions"] = actions
        await self._send_with_ack(payload)

    async def _send_fs_request(self, payload: dict[str, Any]) -> dict[str, Any]:
        """Wysyla zadanie Files i czeka na odpowiadajace fs_res."""
        self._cmd_id += 1
        request_id = self._cmd_id
        payload["id"] = request_id
        fut: asyncio.Future = self.hass.loop.create_future()
        self._pending[request_id] = fut
        try:
            await self._send(payload)
            async with asyncio.timeout(FS_RESPONSE_TIMEOUT):
                response = await fut
        except TimeoutError as err:
            raise HomeAssistantError(
                "Deskmate Files: klient nie odpowiedzial w ciagu 15 sekund"
            ) from err
        except ConnectionError as err:
            raise HomeAssistantError(
                "Deskmate Files: komputer nie jest polaczony"
            ) from err
        finally:
            self._pending.pop(request_id, None)

        if not isinstance(response, dict) or response.get("t") != "fs_res":
            raise HomeAssistantError("Deskmate Files: nieprawidlowa odpowiedz klienta")
        if not response.get("ok"):
            error = str(response.get("error") or "nieznany blad klienta")
            raise HomeAssistantError(f"Deskmate Files: {error}")
        return response

    async def async_fs_list(self, path: str) -> list[dict[str, Any]]:
        """Zwraca wpisy allowlistowanego katalogu klienta."""
        response = await self._send_fs_request(
            {"t": "fs", "op": "list", "path": path}
        )
        entries = response.get("entries")
        if not isinstance(entries, list) or not all(
            isinstance(entry, dict) for entry in entries
        ):
            raise HomeAssistantError("Deskmate Files: brak poprawnej listy wpisow")
        return entries

    async def async_fs_stat(self, path: str) -> dict[str, Any]:
        """Zwraca metadane allowlistowanej sciezki klienta."""
        response = await self._send_fs_request(
            {"t": "fs", "op": "stat", "path": path}
        )
        stat = response.get("stat")
        if not isinstance(stat, dict):
            raise HomeAssistantError("Deskmate Files: brak poprawnych metadanych")
        return stat

    async def async_fs_read(
        self, path: str, offset: int, len: int
    ) -> dict[str, Any]:
        """Zwraca zakodowany chunk pliku i znacznik EOF."""
        if offset < 0 or len < 0 or len > 256 * 1024:
            raise HomeAssistantError("Deskmate Files: nieprawidlowy zakres odczytu")
        response = await self._send_fs_request(
            {
                "t": "fs",
                "op": "read",
                "path": path,
                "offset": offset,
                "len": len,
            }
        )
        if not isinstance(response.get("data"), str) or not isinstance(
            response.get("eof"), bool
        ):
            raise HomeAssistantError("Deskmate Files: nieprawidlowy fragment pliku")
        return {"data": response["data"], "eof": response["eof"]}

    async def async_shutdown(self) -> None:
        await self._close_current()
        self._fail_pending("shutdown")
