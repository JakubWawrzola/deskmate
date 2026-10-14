"""Sensory Deskmate Link (deklarowane przez klienta)."""
from __future__ import annotations

from homeassistant.components.sensor import SensorDeviceClass, SensorEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback
from homeassistant.util import dt as dt_util

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
        # A timestamp sensor must hand Home Assistant a datetime, not the ISO
        # text the computer sends (e.g. "Last active").
        if self.device_class == SensorDeviceClass.TIMESTAMP:
            return dt_util.parse_datetime(self._value) if isinstance(self._value, str) else None
        # Liczbowy sensor bez wartosci (np. "Screen time left" bez limitu)
        if self._value == "" and self.native_unit_of_measurement:
            return None
        return self._value


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "sensor", DeskmateSensor)
