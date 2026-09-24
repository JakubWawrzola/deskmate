"""Deskmate Files: pliki miedzy Home Assistantem a komputerem.

- Strona "Deskmate Files" w pasku bocznym (tylko administratorzy): wysylanie
  plikow z telefonu/laptopa na komputer i pobieranie plikow z folderow, ktore
  komputer udostepnil do odczytu.
- API HTTP z autoryzacja Home Assistanta. Wysylanie idzie kawalkami po 8 MiB
  (tunel Cloudflare odrzuca ciala zadan powyzej 100 MB), a kazdy kawalek
  trafia na komputer zaszyfrowanym kanalem Link po 256 KiB.
- Uslugi deskmate_link.send_file / fetch_file do automatyzacji, tylko dla
  sciezek z allowlist_external_dirs.

Komputer sam pilnuje, gdzie wolno pisac i czytac; ta strona niczego nie
rozszerza, tylko przenosi dane.
"""
from __future__ import annotations

import asyncio
import base64
from dataclasses import dataclass, field
import hashlib
from http import HTTPStatus
import logging
import os
from pathlib import Path
import time
from typing import Any
from urllib.parse import quote

from aiohttp import web
import voluptuous as vol

from homeassistant.components import frontend, panel_custom
from homeassistant.components.http import HomeAssistantView, StaticPathConfig
from homeassistant.core import HomeAssistant, ServiceCall, ServiceResponse, SupportsResponse
from homeassistant.exceptions import HomeAssistantError, Unauthorized
import homeassistant.helpers.config_validation as cv

from .const import (
    DOMAIN,
    FILES_LINK_CHUNK,
    FILES_HTTP_CHUNK,
    FILES_PANEL_ICON,
    FILES_PANEL_TITLE,
    FILES_PANEL_URL,
    FILES_PANEL_VERSION,
    FILES_STATIC_URL,
    FILES_UPLOAD_IDLE_S,
    SERVICE_FETCH_FILE,
    SERVICE_SEND_FILE,
)

_LOGGER = logging.getLogger(__name__)

PANEL_COMPONENT = "deskmate-files-panel"


@dataclass
class UploadSession:
    """Wysylka z przegladarki w toku (po stronie HA)."""

    entry_id: str
    user_id: str
    name: str
    size: int
    offset: int = 0
    sha: Any = field(default_factory=hashlib.sha256)
    last: float = field(default_factory=time.monotonic)
    lock: asyncio.Lock = field(default_factory=asyncio.Lock)


def _data(hass: HomeAssistant) -> dict:
    return hass.data.setdefault(DOMAIN, {"hubs": {}, "view_registered": False})


def _hubs(hass: HomeAssistant) -> dict:
    return _data(hass).get("hubs", {})


def _uploads(hass: HomeAssistant) -> dict[str, UploadSession]:
    return _data(hass).setdefault("uploads", {})


def _device_name(hub) -> str:
    return (hub.device_info or {}).get("name") or hub.entry.title or hub.node_id


# ── rejestracja ─────────────────────────────────────────────────


async def async_setup_files(hass: HomeAssistant) -> None:
    """Widoki HTTP i pliki statyczne raz na uruchomienie HA, panel gdy trzeba."""
    data = _data(hass)
    if not data.get("files_http"):
        hass.http.register_view(FilesDevicesView())
        hass.http.register_view(FilesActionView())
        await hass.http.async_register_static_paths(
            [
                StaticPathConfig(
                    FILES_STATIC_URL,
                    str(Path(__file__).parent / "frontend"),
                    cache_headers=False,
                )
            ]
        )
        data["files_http"] = True
    if not data.get("files_panel"):
        await panel_custom.async_register_panel(
            hass,
            webcomponent_name=PANEL_COMPONENT,
            frontend_url_path=FILES_PANEL_URL,
            module_url=f"{FILES_STATIC_URL}/{PANEL_COMPONENT}.js?v={FILES_PANEL_VERSION}",
            sidebar_title=FILES_PANEL_TITLE,
            sidebar_icon=FILES_PANEL_ICON,
            require_admin=True,
            config={},
        )
        data["files_panel"] = True
    _register_services(hass)


def async_unload_files(hass: HomeAssistant) -> None:
    """Po odlaczeniu ostatniego komputera znika panel i uslugi."""
    data = _data(hass)
    if data.get("hubs"):
        return
    if data.get("files_panel"):
        frontend.async_remove_panel(hass, FILES_PANEL_URL)
        data["files_panel"] = False
    for service in (SERVICE_SEND_FILE, SERVICE_FETCH_FILE):
        if hass.services.has_service(DOMAIN, service):
            hass.services.async_remove(DOMAIN, service)


# ── wspolne ─────────────────────────────────────────────────────


def _admin_error(request: web.Request) -> web.Response | None:
    user = request.get("hass_user")
    if user is None or not user.is_admin:
        return web.json_response(
            {"message": "Deskmate Files is available to administrators only"},
            status=HTTPStatus.FORBIDDEN,
        )
    return None


def _json_error(message: str, status: HTTPStatus = HTTPStatus.BAD_REQUEST) -> web.Response:
    return web.json_response({"message": message}, status=status)


def _strip_prefix(message: str) -> str:
    return message.removeprefix("Deskmate Files: ")


def _sweep_uploads(hass: HomeAssistant) -> None:
    now = time.monotonic()
    stale = [
        token
        for token, session in _uploads(hass).items()
        if now - session.last > FILES_UPLOAD_IDLE_S and not session.lock.locked()
    ]
    for token in stale:
        session = _uploads(hass).pop(token)
        hub = _hubs(hass).get(session.entry_id)
        if hub is not None:
            hass.async_create_task(hub.async_put_abort(token))


async def _send_chunks(hub, token: str, offset: int, data: bytes, sha) -> int:
    """Przekazuje dane na komputer kawalkami po 256 KiB; zwraca nowy offset."""
    view = memoryview(data)
    for start in range(0, len(view), FILES_LINK_CHUNK):
        piece = bytes(view[start : start + FILES_LINK_CHUNK])
        await hub.async_put_chunk(token, offset, piece)
        sha.update(piece)
        offset += len(piece)
    return offset


# ── widoki HTTP ─────────────────────────────────────────────────


class FilesDevicesView(HomeAssistantView):
    """Lista komputerow, z ktorymi mozna wymieniac pliki."""

    url = "/api/deskmate_link/files"
    name = "api:deskmate_link:files"
    requires_auth = True

    async def get(self, request: web.Request) -> web.Response:
        if (denied := _admin_error(request)) is not None:
            return denied
        hass: HomeAssistant = request.app["hass"]
        devices = [
            {
                "entry_id": entry_id,
                "name": _device_name(hub),
                "node_id": hub.node_id,
                "connected": hub.available,
            }
            for entry_id, hub in _hubs(hass).items()
            if hub.node_id
        ]
        return self.json(devices)


class FilesActionView(HomeAssistantView):
    """Operacje na plikach jednego komputera."""

    url = "/api/deskmate_link/files/{entry_id}/{action}"
    name = "api:deskmate_link:files:action"
    requires_auth = True

    def _hub(self, request: web.Request, entry_id: str):
        hub = _hubs(request.app["hass"]).get(entry_id)
        if hub is None:
            raise web.HTTPNotFound()
        return hub

    async def get(self, request: web.Request, entry_id: str, action: str) -> web.StreamResponse:
        if (denied := _admin_error(request)) is not None:
            return denied
        hub = self._hub(request, entry_id)
        try:
            if action == "roots":
                return self.json(await hub.async_fs_roots())
            if action == "list":
                entries = await hub.async_fs_list(request.query.get("path", ""))
                return self.json({"entries": entries})
            if action == "download":
                return await self._download(request, hub)
        except HomeAssistantError as err:
            return _json_error(_strip_prefix(str(err)), HTTPStatus.BAD_GATEWAY)
        raise web.HTTPNotFound()

    async def _download(self, request: web.Request, hub) -> web.StreamResponse:
        path = request.query.get("path", "")
        stat = await hub.async_fs_stat(path)
        if stat.get("dir"):
            return _json_error("This is a folder, not a file")
        name = str(stat.get("name") or "file")
        size = int(stat.get("size") or 0)
        response = web.StreamResponse(
            headers={
                "Content-Type": "application/octet-stream",
                "Content-Disposition": f"attachment; filename*=UTF-8''{quote(name)}",
                "Cache-Control": "no-store",
                "X-Content-Type-Options": "nosniff",
            }
        )
        response.content_length = size
        await response.prepare(request)
        offset = 0
        try:
            while offset < size:
                chunk = await hub.async_fs_read(path, offset, FILES_LINK_CHUNK)
                data = base64.b64decode(chunk["data"])
                if not data:
                    break
                await response.write(data)
                offset += len(data)
                if chunk["eof"]:
                    break
        except (HomeAssistantError, ConnectionResetError) as err:
            # Naglowki juz wyszly - zerwanie polaczenia to jedyny uczciwy sygnal.
            _LOGGER.warning("deskmate_link: przerwane pobieranie %s: %s", name, err)
            if request.transport is not None:
                request.transport.close()
            return response
        await response.write_eof()
        return response

    async def post(self, request: web.Request, entry_id: str, action: str) -> web.Response:
        if (denied := _admin_error(request)) is not None:
            return denied
        hass: HomeAssistant = request.app["hass"]
        hub = self._hub(request, entry_id)
        user = request["hass_user"]
        try:
            if action == "upload_start":
                return await self._upload_start(request, hass, hub, entry_id, user)
            if action == "upload_chunk":
                return await self._upload_chunk(request, hass, hub, entry_id, user)
            if action == "upload_finish":
                return await self._upload_finish(request, hass, hub, entry_id, user)
            if action == "upload_abort":
                body = await request.json()
                session = _uploads(hass).pop(str(body.get("upload", "")), None)
                if session is not None and session.user_id == user.id:
                    await hub.async_put_abort(str(body.get("upload")))
                return self.json({"ok": True})
        except HomeAssistantError as err:
            return _json_error(_strip_prefix(str(err)), HTTPStatus.BAD_GATEWAY)
        except ValueError:
            return _json_error("Invalid request")
        raise web.HTTPNotFound()

    async def _upload_start(self, request, hass, hub, entry_id, user) -> web.Response:
        _sweep_uploads(hass)
        body = await request.json()
        name = body.get("name")
        size = body.get("size")
        if not isinstance(name, str) or not name or len(name) > 255:
            return _json_error("Invalid file name")
        if not isinstance(size, int) or isinstance(size, bool) or size < 0:
            return _json_error("Invalid file size")
        result = await hub.async_put_begin(
            name, size, f"Home Assistant user {user.name or 'unknown'}"
        )
        token = str(result.get("upload", ""))
        _uploads(hass)[token] = UploadSession(
            entry_id=entry_id, user_id=user.id, name=str(result.get("name", name)), size=size
        )
        _LOGGER.info(
            "deskmate_link[%s]: %s wysyla %s (%s B)", hub.node_id, user.name, name, size
        )
        return self.json(
            {"upload": token, "name": result.get("name", name), "chunk_bytes": FILES_HTTP_CHUNK}
        )

    def _session(self, hass, token: str, entry_id: str, user) -> UploadSession | None:
        session = _uploads(hass).get(token)
        if session is None or session.entry_id != entry_id or session.user_id != user.id:
            return None
        return session

    async def _fail(self, hass, hub, token: str, message: str, status=HTTPStatus.BAD_REQUEST):
        _uploads(hass).pop(token, None)
        await hub.async_put_abort(token)
        return _json_error(message, status)

    async def _upload_chunk(self, request, hass, hub, entry_id, user) -> web.Response:
        token = request.query.get("upload", "")
        session = self._session(hass, token, entry_id, user)
        if session is None:
            return _json_error("Unknown or expired upload", HTTPStatus.NOT_FOUND)
        offset = int(request.query.get("offset", "-1"))
        async with session.lock:
            if offset != session.offset:
                return web.json_response(
                    {"message": "Unexpected offset", "offset": session.offset},
                    status=HTTPStatus.CONFLICT,
                )
            buffer = bytearray()
            received = 0
            try:
                async for data in request.content.iter_chunked(64 * 1024):
                    received += len(data)
                    if received > FILES_HTTP_CHUNK:
                        return await self._fail(
                            hass, hub, token, "Chunk too large", HTTPStatus.REQUEST_ENTITY_TOO_LARGE
                        )
                    if session.offset + len(buffer) + len(data) > session.size:
                        return await self._fail(hass, hub, token, "More data than announced")
                    buffer.extend(data)
                    if len(buffer) >= FILES_LINK_CHUNK:
                        whole = len(buffer) - len(buffer) % FILES_LINK_CHUNK
                        session.offset = await _send_chunks(
                            hub, token, session.offset, bytes(buffer[:whole]), session.sha
                        )
                        del buffer[:whole]
                if buffer:
                    session.offset = await _send_chunks(
                        hub, token, session.offset, bytes(buffer), session.sha
                    )
            except HomeAssistantError as err:
                return await self._fail(
                    hass, hub, token, _strip_prefix(str(err)), HTTPStatus.BAD_GATEWAY
                )
            session.last = time.monotonic()
            return self.json({"received": session.offset})

    async def _upload_finish(self, request, hass, hub, entry_id, user) -> web.Response:
        body = await request.json()
        token = str(body.get("upload", ""))
        session = self._session(hass, token, entry_id, user)
        if session is None:
            return _json_error("Unknown or expired upload", HTTPStatus.NOT_FOUND)
        async with session.lock:
            if session.offset != session.size:
                return await self._fail(hass, hub, token, "Upload incomplete")
            _uploads(hass).pop(token, None)
            result = await hub.async_put_end(token, session.sha.hexdigest())
        _LOGGER.info(
            "deskmate_link[%s]: zapisano %s w %s", hub.node_id, result.get("name"), result.get("dir")
        )
        return self.json(
            {"name": result.get("name"), "dir": result.get("dir"), "size": result.get("size")}
        )


# ── uslugi ──────────────────────────────────────────────────────

SEND_FILE_SCHEMA = vol.Schema(
    {
        vol.Optional("node_id"): cv.string,
        vol.Required("path"): cv.string,
        vol.Optional("filename"): cv.string,
    }
)
FETCH_FILE_SCHEMA = vol.Schema(
    {
        vol.Optional("node_id"): cv.string,
        vol.Required("path"): cv.string,
        vol.Required("destination"): cv.string,
        vol.Optional("overwrite", default=False): cv.boolean,
    }
)


def _select_hub(hass: HomeAssistant, node_id: str | None):
    hubs = [hub for hub in _hubs(hass).values() if hub.node_id]
    if node_id:
        hubs = [hub for hub in hubs if hub.node_id == node_id]
    if not hubs:
        raise HomeAssistantError(f"deskmate_link: brak sparowanego komputera {node_id!r}")
    if len(hubs) > 1:
        raise HomeAssistantError("deskmate_link: kilka komputerow - podaj node_id")
    return hubs[0]


async def _check_admin(hass: HomeAssistant, call: ServiceCall) -> str:
    """Automatyzacje (bez uzytkownika) moga; z UI tylko administrator."""
    if not call.context.user_id:
        return "a Home Assistant automation"
    user = await hass.auth.async_get_user(call.context.user_id)
    if user is None or not user.is_admin:
        raise Unauthorized()
    return f"Home Assistant user {user.name}"


def _register_services(hass: HomeAssistant) -> None:
    async def send_file(call: ServiceCall) -> ServiceResponse:
        return await _async_send_file(hass, call)

    async def fetch_file(call: ServiceCall) -> ServiceResponse:
        return await _async_fetch_file(hass, call)

    if not hass.services.has_service(DOMAIN, SERVICE_SEND_FILE):
        hass.services.async_register(
            DOMAIN,
            SERVICE_SEND_FILE,
            send_file,
            schema=SEND_FILE_SCHEMA,
            supports_response=SupportsResponse.OPTIONAL,
        )
    if not hass.services.has_service(DOMAIN, SERVICE_FETCH_FILE):
        hass.services.async_register(
            DOMAIN,
            SERVICE_FETCH_FILE,
            fetch_file,
            schema=FETCH_FILE_SCHEMA,
            supports_response=SupportsResponse.OPTIONAL,
        )


async def _async_send_file(hass: HomeAssistant, call: ServiceCall) -> ServiceResponse:
    """Plik z Home Assistanta (np. zdjecie z kamery) do skrzynki komputera."""
    by = await _check_admin(hass, call)
    hub = _select_hub(hass, call.data.get("node_id"))
    path = call.data["path"]
    if not hass.config.is_allowed_path(path):
        raise HomeAssistantError(
            f"deskmate_link: {path} nie jest w allowlist_external_dirs"
        )
    if not await hass.async_add_executor_job(os.path.isfile, path):
        raise HomeAssistantError(f"deskmate_link: brak pliku {path}")
    size = await hass.async_add_executor_job(os.path.getsize, path)
    name = call.data.get("filename") or os.path.basename(path)
    begin = await hub.async_put_begin(name, size, by)
    token = str(begin.get("upload", ""))
    handle = await hass.async_add_executor_job(open, path, "rb")
    try:
        sha = hashlib.sha256()
        offset = 0
        while True:
            piece = await hass.async_add_executor_job(handle.read, FILES_LINK_CHUNK)
            if not piece:
                break
            offset = await _send_chunks(hub, token, offset, piece, sha)
        result = await hub.async_put_end(token, sha.hexdigest())
    except BaseException:
        await hub.async_put_abort(token)
        raise
    finally:
        await hass.async_add_executor_job(handle.close)
    return {"saved_as": result.get("name"), "folder": result.get("dir")}


async def _async_fetch_file(hass: HomeAssistant, call: ServiceCall) -> ServiceResponse:
    """Plik z udostepnionego folderu komputera do Home Assistanta."""
    await _check_admin(hass, call)
    hub = _select_hub(hass, call.data.get("node_id"))
    source = call.data["path"]
    stat = await hub.async_fs_stat(source)
    if stat.get("dir"):
        raise HomeAssistantError("deskmate_link: to folder, nie plik")
    destination = call.data["destination"]
    if await hass.async_add_executor_job(os.path.isdir, destination):
        safe = os.path.basename(str(stat.get("name") or "file").replace("\\", "/")) or "file"
        destination = os.path.join(destination, safe)
    if not hass.config.is_allowed_path(destination):
        raise HomeAssistantError(
            f"deskmate_link: {destination} nie jest w allowlist_external_dirs"
        )
    if not call.data["overwrite"] and await hass.async_add_executor_job(
        os.path.exists, destination
    ):
        raise HomeAssistantError(f"deskmate_link: {destination} juz istnieje")
    size = int(stat.get("size") or 0)
    part = f"{destination}.part"
    handle = await hass.async_add_executor_job(open, part, "wb")
    offset = 0
    try:
        while offset < size:
            chunk = await hub.async_fs_read(source, offset, FILES_LINK_CHUNK)
            data = base64.b64decode(chunk["data"])
            if not data:
                break
            await hass.async_add_executor_job(handle.write, data)
            offset += len(data)
            if chunk["eof"]:
                break
    except BaseException:
        await hass.async_add_executor_job(handle.close)
        await hass.async_add_executor_job(os.remove, part)
        raise
    await hass.async_add_executor_job(handle.close)
    await hass.async_add_executor_job(os.replace, part, destination)
    return {"path": destination, "size": offset}
