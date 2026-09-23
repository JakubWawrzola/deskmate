"""Sensory Deskmate Link (deklarowane przez klienta)."""
from __future__ import annotations

from homeassistant.components.sensor import SensorEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateEntity, setup_dynamic_platform


class DeskmateSensor(DeskmateEntity, SensorEntity):
    def __init__(self, hub, desc) -> None:
        super().__init__(hub, desc)
        if desc.get("unit"):
            self._attr_native_unit_of_measurement = desc["unit"]
        if desc.get("device_class"):
            self._attr_device_class = desc["device_class"]
        if desc.get("state_class"):
            self._attr_state_class = desc["state_class"]

    @property
    def native_value(self):
        return self._value


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "sensor", DeskmateSensor)
