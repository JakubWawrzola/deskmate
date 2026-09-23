"""Baza encji Deskmate Link + fabryka dynamicznego dodawania po declare."""
from __future__ import annotations

from typing import Any, Callable

from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from homeassistant.helpers.entity import DeviceInfo, Entity

from .const import DOMAIN, SIGNAL_AVAILABLE, SIGNAL_DECLARED, SIGNAL_STATE
from .hub import DeskmateHub


class DeskmateEntity(Entity):
    """Wspolna baza: device, dostepnosc, stan z hub.states."""

    _attr_should_poll = False
    _attr_has_entity_name = True

    def __init__(self, hub: DeskmateHub, desc: dict[str, Any]) -> None:
        self.hub = hub
        self.desc = desc
        self.key: str = desc["key"]
        self._attr_unique_id = f"{hub.entry.entry_id}-{self.key}"
        # name identyczne z dawnym MQTT discovery -> te same entity_id
        self._attr_name = desc.get("name", self.key)
        if desc.get("icon"):
            self._attr_icon = desc["icon"]

    @property
    def device_info(self) -> DeviceInfo:
        info = self.hub.device_info
        return DeviceInfo(
            identifiers={(DOMAIN, self.hub.node_id)},
            name=info.get("name", self.hub.node_id),
            model=info.get("model", "Deskmate"),
            sw_version=info.get("sw_version"),
            manufacturer="Deskmate",
        )

    @property
    def available(self) -> bool:
        return self.hub.available

    @property
    def _value(self) -> Any:
        return self.hub.states.get(self.key)

    async def async_added_to_hass(self) -> None:
        eid = self.hub.entry.entry_id
        for signal in (f"{SIGNAL_STATE}_{eid}", f"{SIGNAL_AVAILABLE}_{eid}"):
            self.async_on_remove(
                async_dispatcher_connect(self.hass, signal, self._on_update)
            )

    @callback
    def _on_update(self) -> None:
        self.async_write_ha_state()


def setup_dynamic_platform(
    hass: HomeAssistant,
    entry,
    async_add_entities,
    kind: str,
    factory: Callable[[DeskmateHub, dict[str, Any]], DeskmateEntity],
) -> None:
    """Dodaje encje danego rodzaju po kazdym declare (bez duplikatow)."""
    hub: DeskmateHub = hass.data[DOMAIN]["hubs"][entry.entry_id]
    added: set[str] = set()

    @callback
    def _sync() -> None:
        declared = {d["key"] for d in hub.entities if d.get("kind") == kind and d.get("key")}
        # klucz zniknal z declare -> hub usunal go z rejestru; pozwol dodac ponownie
        added.intersection_update(declared)
        new = []
        for desc in hub.entities:
            if desc.get("kind") != kind or desc.get("key") in added:
                continue
            added.add(desc["key"])
            new.append(factory(hub, desc))
        if new:
            async_add_entities(new)

    entry.async_on_unload(
        async_dispatcher_connect(hass, f"{SIGNAL_DECLARED}_{entry.entry_id}", _sync)
    )
    _sync()  # encje z declare sprzed zaladowania platformy
