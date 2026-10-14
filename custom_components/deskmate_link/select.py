"""Listy wyboru Deskmate Link (urzadzenia dzwieku, akcja po czasie ekranowym)."""
from __future__ import annotations

from homeassistant.components.select import SelectEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import SIGNAL_DECLARED
from .entity import DeskmateOwnedEntity, setup_dynamic_platform


class DeskmateSelect(DeskmateOwnedEntity, SelectEntity):
    def __init__(self, hub, desc) -> None:
        super().__init__(hub, desc)
        self._attr_options = [str(option) for option in desc.get("options", [])]

    async def async_added_to_hass(self) -> None:
        await super().async_added_to_hass()
        # Podlaczone lub odlaczone urzadzenie: komputer wysyla declare z nowymi opcjami
        self.async_on_remove(
            async_dispatcher_connect(
                self.hass,
                f"{SIGNAL_DECLARED}_{self.hub.entry.entry_id}",
                self._on_declared,
            )
        )

    @callback
    def _on_declared(self) -> None:
        options = [str(option) for option in self._current_desc().get("options", [])]
        if options != self.options:
            self._attr_options = options
            self.async_write_ha_state()

    @property
    def current_option(self) -> str | None:
        value = self._value
        return value if isinstance(value, str) and value in self.options else None

    async def async_select_option(self, option: str) -> None:
        await self._set_owned(option)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(hass, entry, async_add_entities, "select", DeskmateSelect)
