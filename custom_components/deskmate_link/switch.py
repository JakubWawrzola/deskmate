"""Przelaczniki Deskmate Link (np. keep_awake)."""
from __future__ import annotations

from typing import Any

from homeassistant.components.switch import SwitchEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateEntity, setup_dynamic_platform


class DeskmateSwitch(DeskmateEntity, SwitchEntity):
    @property
    def is_on(self):
        value = self._value
        if value is None:
            return None
        return bool(value)

    async def async_turn_on(self, **kwargs: Any) -> None:
        await self.hub.async_command(self.key, "set", True)

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self.hub.async_command(self.key, "set", False)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "switch", DeskmateSwitch)
