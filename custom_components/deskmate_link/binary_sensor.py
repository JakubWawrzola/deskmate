"""Sensory binarne Deskmate Link."""
from __future__ import annotations

from homeassistant.components.binary_sensor import BinarySensorEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateEntity, setup_dynamic_platform


class DeskmateBinarySensor(DeskmateEntity, BinarySensorEntity):
    def __init__(self, hub, desc) -> None:
        super().__init__(hub, desc)
        if desc.get("device_class"):
            self._attr_device_class = desc["device_class"]

    @property
    def is_on(self):
        value = self._value
        if value is None:
            return None
        return bool(value)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(
        hass, entry, async_add_entities, "binary_sensor", DeskmateBinarySensor
    )
