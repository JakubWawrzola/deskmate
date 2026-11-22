"""Deskmate Link - szyfrowany kanal Deskmate <-> HA bez MQTT."""

from __future__ import annotations

import logging

import homeassistant.helpers.config_validation as cv
import voluptuous as vol
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, ServiceCall
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers.storage import Store

from .const import (
    CONF_NODE_ID,
    DOMAIN,
    LEGACY_UNBOUND_TITLE,
    PLATFORMS,
    SERVICE_CLEAR_NOTIFICATION,
    SERVICE_NOTIFY,
    STORAGE_VERSION,
    UNBOUND_TITLE,
)
from .files import async_setup_files, async_unload_files
from .hub import DeskmateHub
from .notify_tag import (
    NOTIFY_FAILURES,
    NOTIFY_IMAGE_PLACEMENTS,
    NOTIFY_PRIORITIES,
    clear_failures_error,
    notify_failures_error,
    validate_tag,
)
from .service_role import refuse_service_only
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
        vol.Optional("image_placement"): vol.In(NOTIFY_IMAGE_PLACEMENTS),
        vol.Optional("actions"): [
            vol.Schema(
                {vol.Required("id"): cv.string, vol.Required("title"): cv.string}
            )
        ],
        vol.Optional("tag"): vol.All(cv.string, validate_tag),
        vol.Optional("priority"): vol.In(NOTIFY_PRIORITIES),
    }
)

CLEAR_NOTIFICATION_SCHEMA = vol.Schema(
    {
        vol.Optional("node_id"): cv.string,
        vol.Required("tag"): vol.All(cv.string, validate_tag),
    }
)


def _domain_data(hass: HomeAssistant) -> dict:
    return hass.data.setdefault(DOMAIN, {"hubs": {}, "view_registered": False})


def _notify_targets(hass: HomeAssistant, node_id: str | None) -> list[DeskmateHub]:
    hubs: dict[str, DeskmateHub] = _domain_data(hass)["hubs"]
    # Bez node_id: tylko komputery. Telewizor (DeskmateTV) dostaje
    # dymek wylacznie przy jawnym node_id, inaczej toast "na komputer"
    # wyskakiwalby tez na ekranie TV. An entry still waiting for pairing has
    # no computer to reach, so it is never a target.
    targets = [
        h
        for h in hubs.values()
        if h.node_id and ((node_id is None and not h.is_tv) or h.node_id == node_id)
    ]
    if not targets:
        if node_id is None:
            raise HomeAssistantError("Deskmate: no paired computer")
        raise HomeAssistantError(
            f"Deskmate: no paired computer with node id {node_id!r}"
        )
    return targets


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
            # Every computer is tried: one that is off or has only the service
            # connected must not keep the notification from the ones after it.
            targets = _notify_targets(hass, call.data.get("node_id"))
            failures: list[tuple[str, Exception]] = []
            for h in targets:
                try:
                    refuse_service_only(h)
                    image = call.data.get("image")
                    if h.is_tv and image and image.startswith("camera."):
                        # TV nie ma tokenu do HA - klatka z kamery idzie w wiadomosci
                        image = await async_camera_data_uri(hass, image)
                    await h.async_notify(
                        call.data["title"],
                        call.data["message"],
                        image,
                        call.data.get("actions"),
                        call.data.get("tag"),
                        call.data.get("priority"),
                        call.data.get("image_placement"),
                    )
                except NOTIFY_FAILURES as err:
                    failures.append((h.node_id, err))
            error = notify_failures_error(len(targets), failures)
            if error is not None:
                raise error

        hass.services.async_register(
            DOMAIN, SERVICE_NOTIFY, _notify, schema=NOTIFY_SCHEMA
        )

    if not hass.services.has_service(DOMAIN, SERVICE_CLEAR_NOTIFICATION):

        async def _clear_notification(call: ServiceCall) -> None:
            # Every computer is tried: one that is asleep or too old must not
            # leave the notification on the computers after it.
            targets = _notify_targets(hass, call.data.get("node_id"))
            failures: list[tuple[str, HomeAssistantError]] = []
            for h in targets:
                try:
                    refuse_service_only(h)
                    await h.async_notify_clear(call.data["tag"])
                except HomeAssistantError as err:
                    failures.append((h.node_id or h.entry.title, err))
            error = clear_failures_error(len(targets), failures)
            if error is not None:
                raise error

        hass.services.async_register(
            DOMAIN,
            SERVICE_CLEAR_NOTIFICATION,
            _clear_notification,
            schema=CLEAR_NOTIFICATION_SCHEMA,
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
