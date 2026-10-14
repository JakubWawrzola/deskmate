"""Przelaczniki Deskmate Link (np. keep_awake)."""
from __future__ import annotations

from typing import Any

from homeassistant.components.switch import SwitchEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateOwnedEntity, setup_dynamic_platform


def _as_bool(value: Any) -> bool:
    # Deskmate do 0.8.0 wysylal "Mute audio" jako tekst "ON"/"OFF"
    if isinstance(value, str):
        return value.strip().lower() in ("on", "true", "1")
    return bool(value)


class DeskmateSwitch(DeskmateOwnedEntity, SwitchEntity):
    def _normalize(self, value: Any) -> bool:
        return _as_bool(value)

    @property
    def is_on(self):
        value = self._value
        if value is None:
            return None
        return _as_bool(value)

    async def async_turn_on(self, **kwargs: Any) -> None:
        await self._set_owned(True)

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self._set_owned(False)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "switch", DeskmateSwitch)
