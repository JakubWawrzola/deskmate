"""DeskmateTV: wezel typu tv (Android / Google TV) i jego aktualizacje.

Aplikacja na TV (katalog `tv/` repo) mowi tym samym protokolem Link co
Deskmate na PC; dochodza zapytania `{"t":"tv","op":...}` z odpowiedzia
`tv_res` (hub.async_tv_request). ADB na TV zwykle jest po instalacji
wylaczone, wiec nowa wersja APK idzie z HA przez Link: plik z
/config/deskmate_tv/. Gdy lezy tam nowsza wersja niz na TV, HA wysyla ja sam
przy najblizszym polaczeniu (raz na wersje i uruchomienie HA).
Opis: docs/TV.md.
"""
from __future__ import annotations

import asyncio
import base64
import hashlib
import logging
import re
from pathlib import Path
from typing import Any, Callable

import voluptuous as vol

from homeassistant.core import HomeAssistant, ServiceCall, ServiceResponse, SupportsResponse, callback
from homeassistant.exceptions import HomeAssistantError, ServiceValidationError
import homeassistant.helpers.config_validation as cv
from homeassistant.helpers.dispatcher import async_dispatcher_connect

from .const import DOMAIN, SERVICE_TV_UPDATE, SIGNAL_DECLARED, TV_UPDATE_CHUNK, TV_UPDATE_DIR

_LOGGER = logging.getLogger(__name__)

TV_UPDATE_SCHEMA = vol.Schema(
    {
        vol.Optional("node_id"): cv.string,
        vol.Optional("file"): cv.string,
    }
)
AUTO_UPDATE_DELAY_S = 20
_APK_RE = re.compile(r"^DeskmateTV-(\d+(?:\.\d+)*)\.apk$")


def find_tv_hub(hass: HomeAssistant, node_id: str | None = None):
    """Hub DeskmateTV: wskazany node albo jedyny/pierwszy wezel typu tv."""
    hubs = (hass.data.get(DOMAIN) or {}).get("hubs", {})
    candidates = [h for h in hubs.values() if h.is_tv or (node_id and h.node_id == node_id)]
    if node_id:
        candidates = [h for h in candidates if h.node_id == node_id]
    # polaczony wygrywa z odlaczonym duplikatem
    candidates.sort(key=lambda h: not h.available)
    return candidates[0] if candidates else None


def _version(text: str | None) -> tuple[int, ...]:
    parts = re.findall(r"\d+", text or "")
    return tuple(int(p) for p in parts[:4]) or (0,)


def _newest_apk(directory: Path) -> tuple[str, tuple[int, ...]] | None:
    best: tuple[str, tuple[int, ...]] | None = None
    if not directory.is_dir():
        return None
    for path in directory.glob("DeskmateTV-*.apk"):
        m = _APK_RE.match(path.name)
        if not m:
            continue
        version = _version(m.group(1))
        if best is None or version > best[1]:
            best = (path.name, version)
    return best


def _pick_apk(directory: Path, name: str | None) -> Path:
    if name:
        safe = Path(name).name  # bez sciezek - tylko plik z katalogu aktualizacji
        if not safe.endswith(".apk"):
            raise HomeAssistantError("DeskmateTV: update file must be an .apk")
        path = directory / safe
        if not path.is_file():
            raise HomeAssistantError(f"DeskmateTV: no {safe} in /config/{TV_UPDATE_DIR}")
        return path
    newest = _newest_apk(directory)
    if newest is None:
        raise HomeAssistantError(f"DeskmateTV: no DeskmateTV-<version>.apk in /config/{TV_UPDATE_DIR}")
    return directory / newest[0]


def _read_apk(directory: Path, name: str | None) -> tuple[str, bytes]:
    path = _pick_apk(directory, name)
    return path.name, path.read_bytes()


async def async_tv_update(hass: HomeAssistant, hub, name: str | None = None) -> dict[str, Any]:
    """Wysyla APK na TV fragmentami; TV sprawdza SHA-256, wersje i podpis."""
    if not hub.available:
        raise HomeAssistantError("DeskmateTV: the TV is not connected")
    directory = Path(hass.config.path(TV_UPDATE_DIR))
    filename, data = await hass.async_add_executor_job(_read_apk, directory, name)
    digest = hashlib.sha256(data).hexdigest()
    _LOGGER.info("DeskmateTV: wysylam %s (%s B) na %s", filename, len(data), hub.node_id)

    async def step(op: str, args: dict[str, Any], timeout: float = 30.0) -> dict[str, Any]:
        res = await hub.async_tv_request(op, args, timeout)
        if not res.get("ok"):
            raise HomeAssistantError(f"DeskmateTV {op}: {res.get('error') or res.get('code')}")
        return res

    await step("update_begin", {"size": len(data), "sha256": digest})
    try:
        for offset in range(0, len(data), TV_UPDATE_CHUNK):
            chunk = data[offset : offset + TV_UPDATE_CHUNK]
            await step(
                "update_chunk",
                {"offset": offset, "data": base64.b64encode(chunk).decode()},
            )
        result = await step("update_end", {"sha256": digest}, timeout=60.0)
    except HomeAssistantError:
        try:
            await hub.async_tv_request("update_abort", {}, 10.0)
        except HomeAssistantError:
            pass
        raise
    _LOGGER.info("DeskmateTV: %s przyjety, instalacja w toku", filename)
    return {
        "file": filename,
        "size": len(data),
        "sha256": digest,
        "status": result.get("status"),
        "version_code": result.get("version_code"),
    }


@callback
def async_track_tv_auto_update(hass: HomeAssistant, hub) -> Callable[[], None]:
    """Po kazdym `declare` TV: jesli w /config/deskmate_tv jest nowsza wersja, wyslij ja."""
    attempted: set[tuple[int, ...]] = set()

    async def _check() -> None:
        if not hub.is_tv or not hub.available:
            return
        directory = Path(hass.config.path(TV_UPDATE_DIR))
        newest = await hass.async_add_executor_job(_newest_apk, directory)
        if newest is None:
            return
        name, version = newest
        current = _version((hub.device_info or {}).get("sw_version"))
        if version <= current or version in attempted:
            return
        attempted.add(version)  # jedna proba na wersje, bez petli przy bledzie
        await asyncio.sleep(AUTO_UPDATE_DELAY_S)
        if not hub.available:
            return
        try:
            result = await async_tv_update(hass, hub, name)
            _LOGGER.info("DeskmateTV[%s]: autoaktualizacja %s -> %s", hub.node_id, current, result.get("status"))
        except HomeAssistantError as err:
            _LOGGER.warning("DeskmateTV[%s]: autoaktualizacja %s nieudana: %s", hub.node_id, name, err)

    @callback
    def _declared() -> None:
        hass.async_create_background_task(_check(), f"deskmate_tv_auto_update_{hub.entry.entry_id}")

    return async_dispatcher_connect(hass, f"{SIGNAL_DECLARED}_{hub.entry.entry_id}", _declared)


async def async_camera_data_uri(hass: HomeAssistant, entity_id: str, width: int = 960) -> str:
    """Klatka z kamery HA jako data URI (TV nie ma tokenu do API HA)."""
    from homeassistant.components.camera import async_get_image

    image = await async_get_image(hass, entity_id, width=width)
    return f"data:{image.content_type};base64,{base64.b64encode(image.content).decode()}"


def async_register_tv_services(hass: HomeAssistant) -> None:
    if hass.services.has_service(DOMAIN, SERVICE_TV_UPDATE):
        return

    async def _update(call: ServiceCall) -> ServiceResponse:
        hub = find_tv_hub(hass, call.data.get("node_id"))
        if hub is None:
            raise ServiceValidationError("DeskmateTV: no paired TV")
        if not hub.available:
            # Zwykla sytuacja (TV w czuwaniu) - komunikat, nie traceback w logu
            raise ServiceValidationError(
                "DeskmateTV: the TV is not connected. A newer APK in "
                f"/config/{TV_UPDATE_DIR} is sent automatically when it connects."
            )
        return await async_tv_update(hass, hub, call.data.get("file"))

    hass.services.async_register(
        DOMAIN,
        SERVICE_TV_UPDATE,
        _update,
        schema=TV_UPDATE_SCHEMA,
        supports_response=SupportsResponse.OPTIONAL,
    )
