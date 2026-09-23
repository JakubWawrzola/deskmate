"""Pola tekstowe Deskmate Link (np. wpis tekstu / tryb prezentacji)."""
from __future__ import annotations

from homeassistant.components.text import TextEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateEntity, setup_dynamic_platform


class DeskmateText(DeskmateEntity, TextEntity):
    def __init__(self, hub, desc) -> None:
        super().__init__(hub, desc)
        self._attr_native_max = int(desc.get("max", 255))
        if desc.get("mode"):
            self._attr_mode = desc["mode"]

    @property
    def native_value(self):
        value = self._value
        return "" if value is None else str(value)

    async def async_set_value(self, value: str) -> None:
        await self.hub.async_command(self.key, "set", value)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "text", DeskmateText)
