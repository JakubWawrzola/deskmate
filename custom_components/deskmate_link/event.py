"""Encje event Deskmate Link - hotkeye i inne zdarzenia z klienta.

Klient deklaruje kind "event" z lista event_types (np. ["press"]) i wysyla
wiadomosc {"t":"trigger","key":...,"event":...}. Kazdy trigger to takze
event `deskmate_link_trigger` na busie HA (dla automatyzacji).
"""
from __future__ import annotations

from homeassistant.components.event import EventEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import SIGNAL_TRIGGER
from .entity import DeskmateEntity, setup_dynamic_platform


class DeskmateEvent(DeskmateEntity, EventEntity):
    def __init__(self, hub, desc) -> None:
        super().__init__(hub, desc)
        self._attr_event_types = list(desc.get("event_types", ["press"]))

    async def async_added_to_hass(self) -> None:
        await super().async_added_to_hass()
        self.async_on_remove(
            async_dispatcher_connect(
                self.hass,
                f"{SIGNAL_TRIGGER}_{self.hub.entry.entry_id}_{self.key}",
                self._on_trigger,
            )
        )

    @callback
    def _on_trigger(self, event_type: str) -> None:
        if event_type not in self._attr_event_types:
            event_type = self._attr_event_types[0]
        self._trigger_event(event_type)
        self.async_write_ha_state()


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "event", DeskmateEvent)
