"""Deskmate Link - szyfrowany kanal Deskmate <-> HA bez MQTT."""
from __future__ import annotations

import logging

import voluptuous as vol

from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, ServiceCall
from homeassistant.exceptions import HomeAssistantError
import homeassistant.helpers.config_validation as cv
from homeassistant.helpers.storage import Store

from .const import (
    CONF_NODE_ID,
    DOMAIN,
    LEGACY_UNBOUND_TITLE,
    PLATFORMS,
    SERVICE_NOTIFY,
    STORAGE_VERSION,
    UNBOUND_TITLE,
)
from .files import async_setup_files, async_unload_files
from .hub import DeskmateHub
from .tv import (
    async_camera_data_uri,
    async_register_tv_services,
    async_track_tv_auto_update,
)
from .ws_view import DeskmateLinkWsView

_LOGGER = logging.getLogger(__name__)

NOTIFY_SCHEMA = vol.Schema(
    {
        vol.Optional("node_id"): cv.string,
        vol.Required("title"): cv.string,
        vol.Required("message"): cv.string,
        vol.Optional("image"): cv.string,
        vol.Optional("actions"): [
            vol.Schema(
                {vol.Required("id"): cv.string, vol.Required("title"): cv.string}
            )
        ],
    }
)


def _domain_data(hass: HomeAssistant) -> dict:
    return hass.data.setdefault(DOMAIN, {"hubs": {}, "view_registered": False})


async def async_setup_entry(hass: HomeAssistant, entry: ConfigEntry) -> bool:
    data = _domain_data(hass)

    # Wpis czekajacy na parowanie mial do 0.7.2 polski tytul takze w angielskim HA.
    if not entry.data.get(CONF_NODE_ID) and entry.title == LEGACY_UNBOUND_TITLE:
        hass.config_entries.async_update_entry(entry, title=UNBOUND_TITLE)

    if not data["view_registered"]:
        hass.http.register_view(DeskmateLinkWsView())
        data["view_registered"] = True

    hub = DeskmateHub(hass, entry)
    # Encje musza istniec zaraz po restarcie HA, takze gdy komputer jest
    # wylaczony - odpowiednik retained discovery w MQTT.
    await hub.async_restore()
    data["hubs"][entry.entry_id] = hub
    entry.async_on_unload(async_track_tv_auto_update(hass, hub))

    if not hass.services.has_service(DOMAIN, SERVICE_NOTIFY):

        async def _notify(call: ServiceCall) -> None:
            hubs: dict[str, DeskmateHub] = _domain_data(hass)["hubs"]
            node_id = call.data.get("node_id")
            # Bez node_id: tylko komputery. Telewizor (DeskmateTV) dostaje
            # dymek wylacznie przy jawnym node_id, inaczej toast "na komputer"
            # wyskakiwalby tez na ekranie TV.
            targets = [
                h
                for h in hubs.values()
                if (node_id is None and not h.is_tv) or h.node_id == node_id
            ]
            if not targets:
                raise HomeAssistantError(
                    f"Deskmate: no paired computer with node id {node_id!r}"
                )
            for h in targets:
                image = call.data.get("image")
                if h.is_tv and image and image.startswith("camera."):
                    # TV nie ma tokenu do HA - klatka z kamery idzie w wiadomosci
                    image = await async_camera_data_uri(hass, image)
                await h.async_notify(
                    call.data["title"],
                    call.data["message"],
                    image,
                    call.data.get("actions"),
                )

        hass.services.async_register(
            DOMAIN, SERVICE_NOTIFY, _notify, schema=NOTIFY_SCHEMA
        )

    # DeskmateTV is not released yet. Its service appears only once a TV node
    # is paired, so computer-only setups never see it.
    if hub.is_tv:
        async_register_tv_services(hass)
    await async_setup_files(hass)
    await hass.config_entries.async_forward_entry_setups(entry, PLATFORMS)
    _LOGGER.info(
        "deskmate_link[%s]: skonfigurowano",
        entry.data.get(CONF_NODE_ID) or "oczekuje na parowanie",
    )
    return True


async def async_unload_entry(hass: HomeAssistant, entry: ConfigEntry) -> bool:
    ok = await hass.config_entries.async_unload_platforms(entry, PLATFORMS)
    if ok:
        hub: DeskmateHub | None = _domain_data(hass)["hubs"].pop(entry.entry_id, None)
        if hub is not None:
            await hub.async_shutdown()
        async_unload_files(hass)
    return ok


async def async_remove_entry(hass: HomeAssistant, entry: ConfigEntry) -> None:
    """Kasuje zapamietana deklaracje encji razem z usunietym urzadzeniem."""
    await Store(hass, STORAGE_VERSION, f"{DOMAIN}.{entry.entry_id}").async_remove()
