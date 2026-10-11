"""Sensory binarne Deskmate Link."""
from __future__ import annotations

from homeassistant.components.binary_sensor import (
    BinarySensorDeviceClass,
    BinarySensorEntity,
)
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


class DeskmateLinkStatus(DeskmateEntity, BinarySensorEntity):
    """Stan polaczenia node'a z HA (deklarowany z `link: true`).

    W odroznieniu od pozostalych encji jest zawsze dostepna: rozlaczenie to
    stan `off`, a nie `unavailable`, wiec automatyzacje moga na nie reagowac.
    """

    _attr_device_class = BinarySensorDeviceClass.CONNECTIVITY

    @property
    def available(self) -> bool:
        return True

    @property
    def is_on(self) -> bool:
        return self.hub.available


def _factory(hub, desc) -> DeskmateEntity:
    if desc.get("link"):
        return DeskmateLinkStatus(hub, desc)
    return DeskmateBinarySensor(hub, desc)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(
        hass, entry, async_add_entities, "binary_sensor", _factory
    )
