"""The Connection sensor: which client is connected, or why none is.

Unlike every other entity, this one belongs to the hub and not to the
client: no `declare` lists it, no `declare` removes it, and it is always
available. Its state comes from `Presence.connection`.
"""

from __future__ import annotations

from homeassistant.components.sensor import SensorDeviceClass, SensorEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.const import EntityCategory
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from homeassistant.helpers.entity import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import CONNECTION_STATES, DOMAIN, SIGNAL_CONNECTION, SIGNAL_DECLARED
from .entity import hub_device_info
from .hub import DeskmateHub
from .presence import connection_unique_id


class DeskmateConnectionSensor(SensorEntity):
    """Who holds the connection of one computer."""

    _attr_should_poll = False
    _attr_has_entity_name = True
    _attr_translation_key = "connection"
    _attr_device_class = SensorDeviceClass.ENUM
    _attr_entity_category = EntityCategory.DIAGNOSTIC
    _attr_options = list(CONNECTION_STATES)

    def __init__(self, hub: DeskmateHub) -> None:
        self.hub = hub
        self._attr_unique_id = connection_unique_id(hub.entry.entry_id)

    @property
    def device_info(self) -> DeviceInfo:
        return hub_device_info(self.hub)

    @property
    def native_value(self) -> str:
        return self.hub.presence.connection

    async def async_added_to_hass(self) -> None:
        self.async_on_remove(
            async_dispatcher_connect(
                self.hass,
                f"{SIGNAL_CONNECTION}_{self.hub.entry.entry_id}",
                self._on_change,
            )
        )

    @callback
    def _on_change(self) -> None:
        self.async_write_ha_state()


def setup_connection_sensor(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    """Add the sensor as soon as the entry knows its computer."""
    hub: DeskmateHub = hass.data[DOMAIN]["hubs"][entry.entry_id]
    added = False

    @callback
    def _add() -> None:
        nonlocal added
        if added:
            return
        added = True
        async_add_entities([DeskmateConnectionSensor(hub)])

    # An entry that waits for pairing has no device to put the sensor on, and
    # the device takes its name from the first declaration.
    entry.async_on_unload(
        async_dispatcher_connect(hass, f"{SIGNAL_DECLARED}_{entry.entry_id}", _add)
    )
    if hub.node_id and hub.declared_scope is not None:
        _add()
