"""Liczby Deskmate Link (np. glosnosc)."""
from __future__ import annotations

from homeassistant.components.number import NumberEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateEntity, setup_dynamic_platform


class DeskmateNumber(DeskmateEntity, NumberEntity):
    def __init__(self, hub, desc) -> None:
        super().__init__(hub, desc)
        self._attr_native_min_value = float(desc.get("min", 0))
        self._attr_native_max_value = float(desc.get("max", 100))
        self._attr_native_step = float(desc.get("step", 1))
        if desc.get("unit"):
            self._attr_native_unit_of_measurement = desc["unit"]

    @property
    def native_value(self):
        return self._value

    async def async_set_native_value(self, value: float) -> None:
        await self.hub.async_command(self.key, "set", value)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "number", DeskmateNumber)
