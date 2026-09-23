"""Diagnostyka Deskmate Link (klucz redagowany)."""
from __future__ import annotations

from homeassistant.components.diagnostics import async_redact_data
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant

from .const import DOMAIN, REDACT


async def async_get_config_entry_diagnostics(
    hass: HomeAssistant, entry: ConfigEntry
) -> dict:
    hub = hass.data.get(DOMAIN, {}).get("hubs", {}).get(entry.entry_id)
    return {
        "entry": async_redact_data(dict(entry.data), REDACT),
        "available": getattr(hub, "available", None),
        "device_info": getattr(hub, "device_info", None),
        "declared_entities": [
            {k: v for k, v in d.items()} for d in getattr(hub, "entities", [])
        ],
        "state_keys": sorted(getattr(hub, "states", {})),
    }
