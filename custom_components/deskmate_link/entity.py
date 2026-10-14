"""Baza encji Deskmate Link + fabryka dynamicznego dodawania po declare."""
from __future__ import annotations

import logging
import time
from typing import Any, Callable

from homeassistant.const import STATE_UNAVAILABLE, STATE_UNKNOWN
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from homeassistant.helpers.entity import DeviceInfo, Entity
from homeassistant.helpers.restore_state import ExtraStoredData, RestoreEntity

from .const import (
    DOMAIN,
    OWNED_PUSH_EVERY_S,
    SIGNAL_AVAILABLE,
    SIGNAL_DECLARED,
    SIGNAL_STATE,
)
from .hub import DeskmateHub

_LOGGER = logging.getLogger(__name__)


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

    def _current_desc(self) -> dict[str, Any]:
        """Opis z ostatniego declare (np. nowe opcje), a gdy go brak - z utworzenia."""
        for desc in self.hub.entities:
            if desc.get("key") == self.key:
                return desc
        return self.desc

    async def async_added_to_hass(self) -> None:
        eid = self.hub.entry.entry_id
        for signal in (f"{SIGNAL_STATE}_{eid}", f"{SIGNAL_AVAILABLE}_{eid}"):
            self.async_on_remove(
                async_dispatcher_connect(self.hass, signal, self._on_update)
            )

    @callback
    def _on_update(self) -> None:
        self.async_write_ha_state()


class _OwnedValue(ExtraStoredData):
    """Wartosc trzymana przez HA, zapisywana niezaleznie od stanu encji."""

    def __init__(self, value: Any) -> None:
        self.value = value

    def as_dict(self) -> dict[str, Any]:
        return {"owned": self.value}


class DeskmateOwnedEntity(DeskmateEntity, RestoreEntity):
    """Wartosc, ktora na chronionym koncie (reguly rodzinne) trzyma Home Assistant.

    Komputer oznacza takie encje w declare jako "restore". HA pamieta ostatnia
    wartosc ustawiona tutaj (takze po restarcie) i odklada ja, gdy komputer
    zglosi inna - np. po recznej edycji pliku ustawien przez dziecko. Bez
    "restore" encja tylko odzwierciedla stan komputera.
    """

    _owned: Any = None
    _last_push: float = 0.0

    def _normalize(self, value: Any) -> Any:
        return "" if value is None else str(value)

    def _restores(self) -> bool:
        return bool(self._current_desc().get("restore"))

    @property
    def extra_restore_state_data(self) -> ExtraStoredData | None:
        # Osobno od stanu: gdy komputer byl offline przy zamykaniu HA, stan
        # encji to "unavailable", a wartosc ma przetrwac i tak.
        return _OwnedValue(self._owned) if self._owned is not None else None

    async def async_added_to_hass(self) -> None:
        await super().async_added_to_hass()
        restored: Any = None
        extra = await self.async_get_last_extra_data()
        if extra is not None:
            restored = extra.as_dict().get("owned")
        if restored is None:
            last = await self.async_get_last_state()
            if last is not None and last.state not in (STATE_UNKNOWN, STATE_UNAVAILABLE):
                restored = last.state
        if restored is not None:
            try:
                self._owned = self._normalize(restored)
            except (TypeError, ValueError):
                self._owned = None

    async def _set_owned(self, value: Any) -> None:
        """Zmiana z HA: zapamietaj i wyslij; przy odmowie wroc do poprzedniej."""
        previous = self._owned
        self._owned = self._normalize(value)
        try:
            await self.hub.async_command(self.key, "set", value)
        except Exception:
            self._owned = previous
            raise

    @callback
    def _on_update(self) -> None:
        self._keep_owned_value()
        super()._on_update()

    @callback
    def _keep_owned_value(self) -> None:
        reported = self._value
        if reported is None or not self.hub.available:
            return
        try:
            reported = self._normalize(reported)
        except (TypeError, ValueError):
            return
        if self._owned is None or not self._restores():
            # bez ochrony: tylko sledzimy komputer
            self._owned = reported
            return
        if reported == self._owned:
            return
        now = time.monotonic()
        if now - self._last_push < OWNED_PUSH_EVERY_S:
            return
        self._last_push = now
        self.hass.async_create_task(self._push_owned(self._owned))

    async def _push_owned(self, value: Any) -> None:
        try:
            await self.hub.async_command(self.key, "set", value)
        except Exception as err:  # noqa: BLE001 - komputer moze odmowic, sprobujemy pozniej
            _LOGGER.debug(
                "deskmate_link[%s]: %s not restored: %s", self.hub.node_id, self.key, err
            )


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
