"""Hub sesji Deskmate Link - jedno urzadzenie (node) per config entry."""
from __future__ import annotations

import asyncio
import base64
import json
import logging
import secrets
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
    CONF_MIN_VERSION,
    CONF_NODE_ID,
    DIR_C2S,
    DIR_S2C,
    DOMAIN,
    EVENT_NOTIFY_ACTION,
    EVENT_TRIGGER,
    HANDSHAKE_MAX_SKEW_S,
    PERSIST_DELAY_S,
    PROTO_V1,
    PROTO_V2,
    REJECT_AUTH,
    REJECT_CASCADE,
    REJECT_VERSION,
    SIGNAL_AVAILABLE,
    SIGNAL_DECLARED,
    SIGNAL_STATE,
    SIGNAL_TRIGGER,
    STORAGE_VERSION,
)
from .crypto import (
    EphemeralKey,
    FrameCodec,
    b64d_exact,
    derive_cascade_keys,
    derive_session_keys,
    gen_nonce16,
    hs_mac,
    hs_mac_ok,
    v2_cascade_keys,
    v2_hello_bytes,
    v2_mac,
    v2_mac_ok,
    v2_session_keys,
    v2_welcome_bytes,
)

_LOGGER = logging.getLogger(__name__)

CMD_ACK_TIMEOUT = 10.0
FS_RESPONSE_TIMEOUT = 15.0
# Czas na klikniecie "Akceptuj" na komputerze przy odbiorze pliku.
FS_CONFIRM_TIMEOUT = 180.0

# Wynik udanego handshake'u: (welcome, kodek c2s, kodek s2c, node, wersja)
HandshakeResult = tuple[dict, FrameCodec, FrameCodec, str, int]


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
        self.min_version: int = int(entry.data.get(CONF_MIN_VERSION, PROTO_V1))

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

    def try_handshake(self, hello: dict) -> HandshakeResult | str | None:
        """Weryfikuje hello (wersja, node i czas sprawdzone juz w widoku WS).

        Zwraca wynik handshake'u, powod odrzucenia (MAC poprawny, ale cos sie
        nie zgadza - klucz pasuje do TEGO wpisu, wiec szukanie konczy sie tu)
        albo None, gdy klucz nie nalezy do tego wpisu.
        NIE zmienia stanu huba - kodeki sesji instaluje dopiero `run_session`.
        """
        node = hello.get("node")
        if not isinstance(node, str) or not node:
            return None
        if self.node_id and node != self.node_id:
            return None
        if hello.get("v") == PROTO_V2:
            return self._handshake_v2(hello, node)
        if hello.get("v") == PROTO_V1:
            return self._handshake_v1(hello, node)
        return None

    def _cascade_matches(self, node: str, client_cascade: bool) -> bool:
        # Kaskada musi byc wlaczona po obu stronach albo po zadnej. Cicha zgoda
        # na slabszy wariant bylaby najgorszym mozliwym zachowaniem.
        if client_cascade == bool(self._cascade_key):
            return True
        _LOGGER.warning(
            "deskmate_link[%s]: niezgodna kaskada (klient=%s, HA=%s)",
            node,
            client_cascade,
            bool(self._cascade_key),
        )
        return False

    def _handshake_v1(self, hello: dict, node: str) -> HandshakeResult | str | None:
        cn = hello.get("cn", "")
        ts = hello.get("ts", 0)
        if not isinstance(cn, str) or not isinstance(ts, (int, float)):
            return None
        if not hs_mac_ok(self._psk, hello.get("mac", ""), "hello", node, cn, str(int(ts))):
            return None
        if self.min_version > PROTO_V1:
            # Ten komputer mowil juz v2 - v1 to proba downgrade'u albo stary klient.
            return REJECT_VERSION
        if b64d_exact(cn, 16) is None or not self._accept_hello_nonce(cn, ts):
            return REJECT_AUTH
        if not self._cascade_matches(node, bool(hello.get("casc"))):
            return REJECT_CASCADE
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
        return welcome, rx, tx, node, PROTO_V1

    def _handshake_v2(self, hello: dict, node: str) -> HandshakeResult | str | None:
        """v2: X25519 + PSK, MAC nad calym transkryptem (docs/LINK.md)."""
        cn_b64 = hello.get("cn")
        cn = b64d_exact(cn_b64, 16)
        client_epk = b64d_exact(hello.get("epk"), 32)
        ts = hello.get("ts")
        casc = hello.get("casc")
        if (
            cn is None
            or client_epk is None
            or not isinstance(ts, int)
            or isinstance(ts, bool)
            or not isinstance(casc, bool)
        ):
            return None
        hello_bytes = v2_hello_bytes(node, cn, ts, client_epk, casc)
        if not v2_mac_ok(self._psk, hello.get("mac"), hello_bytes):
            return None
        if not self._accept_hello_nonce(str(cn_b64), ts):
            return REJECT_AUTH
        if not self._cascade_matches(node, casc):
            return REJECT_CASCADE
        ephemeral = EphemeralKey()
        shared = ephemeral.exchange(client_epk)
        if shared is None:
            return REJECT_AUTH
        sn = secrets.token_bytes(16)
        now = int(time.time())
        welcome_bytes = v2_welcome_bytes(hello_bytes, sn, now, ephemeral.public)
        k_c2s, k_s2c = v2_session_keys(self._psk, shared, hello_bytes, welcome_bytes)
        casc_c2s = casc_s2c = None
        if self._cascade_key:
            casc_c2s, casc_s2c = v2_cascade_keys(
                self._cascade_key, shared, hello_bytes, welcome_bytes
            )
        rx = FrameCodec(k_c2s, DIR_C2S, node, "c2s", casc_c2s, PROTO_V2)
        tx = FrameCodec(k_s2c, DIR_S2C, node, "s2c", casc_s2c, PROTO_V2)
        welcome = {
            "t": "welcome",
            "v": PROTO_V2,
            "sn": base64.b64encode(sn).decode(),
            "ts": now,
            "epk": base64.b64encode(ephemeral.public).decode(),
            "mac": v2_mac(self._psk, welcome_bytes),
        }
        return welcome, rx, tx, node, PROTO_V2

    async def async_accept(self, node_id: str, version: int) -> None:
        """Po udanym handshake'u: przypiecie wpisu, scalenie duplikatow, ratchet v2."""
        data = dict(self.entry.data)
        changed = False
        # Takze przy juz przypietym wpisie: duplikaty sprzed 0.6.0 znikaja przy
        # pierwszym polaczeniu komputera z poprawnym kluczem.
        await self._absorb_duplicates(node_id)
        if self.node_id != node_id:
            self.node_id = node_id
            data[CONF_NODE_ID] = node_id
            changed = True
            _LOGGER.info("deskmate_link: wpis przypiety do node'a %s", node_id)
        if version > self.min_version:
            self.min_version = version
            data[CONF_MIN_VERSION] = version
            changed = True
            _LOGGER.info(
                "deskmate_link[%s]: protokol v%s - starsze wersje beda odrzucane",
                node_id,
                version,
            )
        if changed:
            self.hass.config_entries.async_update_entry(
                self.entry, title=node_id, unique_id=node_id, data=data
            )

    async def _absorb_duplicates(self, node_id: str) -> None:
        """Przejmuje encje innych wpisow tego samego node'a i je usuwa.

        Po nieudanym parowaniu zwykle dodaje sie integracje jeszcze raz. Nowy
        wpis przypinal sie wtedy do tego samego komputera co stary, a HA mial
        dwa wpisy, jedno wspolne urzadzenie i zdublowane encje z `_2`. Encje
        starego wpisu przechodza do nowego z zachowaniem entity_id i historii,
        a stary wpis znika.
        """
        from homeassistant.helpers import device_registry as dr
        from homeassistant.helpers import entity_registry as er

        others = [
            other
            for other in self.hass.config_entries.async_entries(DOMAIN)
            if other.entry_id != self.entry.entry_id
            and (other.data.get(CONF_NODE_ID) == node_id or other.unique_id == node_id)
        ]
        if not others:
            return
        ent_reg = er.async_get(self.hass)
        dev_reg = dr.async_get(self.hass)
        device = dev_reg.async_get_device(identifiers={(DOMAIN, node_id)})
        if device is not None:
            dev_reg.async_update_device(
                device.id, add_config_entry_id=self.entry.entry_id
            )
        for other in others:
            for ent in er.async_entries_for_config_entry(ent_reg, other.entry_id):
                key = ent.unique_id.split("-", 1)[-1]
                new_uid = f"{self.entry.entry_id}-{key}"
                if ent_reg.async_get_entity_id(ent.domain, DOMAIN, new_uid):
                    ent_reg.async_remove(ent.entity_id)
                    continue
                ent_reg.async_update_entity(
                    ent.entity_id,
                    new_unique_id=new_uid,
                    config_entry_id=self.entry.entry_id,
                )
            _LOGGER.warning(
                "deskmate_link[%s]: scalam zdublowany wpis %s (%s) z nowym parowaniem",
                node_id,
                other.entry_id,
                other.title,
            )
            await self.hass.config_entries.async_remove(other.entry_id)

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

    async def _send_fs_request(
        self, payload: dict[str, Any], timeout: float = FS_RESPONSE_TIMEOUT
    ) -> dict[str, Any]:
        """Wysyla zadanie Files i czeka na odpowiadajace fs_res."""
        self._cmd_id += 1
        request_id = self._cmd_id
        payload["id"] = request_id
        fut: asyncio.Future = self.hass.loop.create_future()
        self._pending[request_id] = fut
        try:
            await self._send(payload)
            async with asyncio.timeout(timeout):
                response = await fut
        except TimeoutError as err:
            raise HomeAssistantError(
                f"Deskmate Files: komputer nie odpowiedzial w ciagu {int(timeout)} s"
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

    async def async_fs_roots(self) -> dict[str, Any]:
        """Foldery do odczytu i stan skrzynki odbiorczej na komputerze."""
        response = await self._send_fs_request({"t": "fs", "op": "roots"})
        return {
            "roots": response.get("roots") or [],
            "inbox": response.get("inbox") or {},
            "max_bytes": response.get("max_bytes") or 0,
        }

    async def async_put_begin(self, name: str, size: int, by: str) -> dict[str, Any]:
        """Otwiera zapis pliku w skrzynce odbiorczej komputera.

        Przy trybie "confirm" komputer czeka na klikniecie uzytkownika, stad
        dluzszy limit czasu.
        """
        return await self._send_fs_request(
            {"t": "fs", "op": "put_begin", "name": name, "size": size, "by": by},
            timeout=FS_CONFIRM_TIMEOUT,
        )

    async def async_put_chunk(self, upload: str, offset: int, data: bytes) -> None:
        await self._send_fs_request(
            {
                "t": "fs",
                "op": "put_chunk",
                "upload": upload,
                "offset": offset,
                "data": base64.b64encode(data).decode(),
            }
        )

    async def async_put_end(self, upload: str, sha256: str) -> dict[str, Any]:
        return await self._send_fs_request(
            {"t": "fs", "op": "put_end", "upload": upload, "sha256": sha256},
            timeout=60.0,
        )

    async def async_put_abort(self, upload: str) -> None:
        """Przerywa zapis; komputer kasuje niedokonczony plik. Best effort."""
        try:
            await self._send_fs_request({"t": "fs", "op": "put_abort", "upload": upload})
        except HomeAssistantError:
            pass

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
