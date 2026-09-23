"""Deskmate Link - szyfrowany kanal Deskmate <-> HA bez MQTT."""
from __future__ import annotations

import logging

import voluptuous as vol

from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, ServiceCall
from homeassistant.exceptions import HomeAssistantError
import homeassistant.helpers.config_validation as cv
from homeassistant.helpers.storage import Store

from .const import CONF_NODE_ID, DOMAIN, PLATFORMS, SERVICE_NOTIFY, STORAGE_VERSION
from .hub import DeskmateHub
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

    if not data["view_registered"]:
        hass.http.register_view(DeskmateLinkWsView())
        data["view_registered"] = True

    hub = DeskmateHub(hass, entry)
    # Encje musza istniec zaraz po restarcie HA, takze gdy komputer jest
    # wylaczony - odpowiednik retained discovery w MQTT.
    await hub.async_restore()
    data["hubs"][entry.entry_id] = hub

    if not hass.services.has_service(DOMAIN, SERVICE_NOTIFY):

        async def _notify(call: ServiceCall) -> None:
            hubs: dict[str, DeskmateHub] = _domain_data(hass)["hubs"]
            node_id = call.data.get("node_id")
            targets = [
                h
                for h in hubs.values()
                if node_id is None or h.node_id == node_id
            ]
            if not targets:
                raise HomeAssistantError(
                    f"deskmate_link: brak sparowanego node'a {node_id!r}"
                )
            for h in targets:
                await h.async_notify(
                    call.data["title"],
                    call.data["message"],
                    call.data.get("image"),
                    call.data.get("actions"),
                )

        hass.services.async_register(
            DOMAIN, SERVICE_NOTIFY, _notify, schema=NOTIFY_SCHEMA
        )

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
    return ok


async def async_remove_entry(hass: HomeAssistant, entry: ConfigEntry) -> None:
    """Kasuje zapamietana deklaracje encji razem z usunietym urzadzeniem."""
    await Store(hass, STORAGE_VERSION, f"{DOMAIN}.{entry.entry_id}").async_remove()
