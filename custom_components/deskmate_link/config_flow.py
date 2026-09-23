"""Config flow: parowanie node'a Deskmate.

Parowanie NIE pyta juz o node_id. HA generuje klucz, a wpis przypina sie do
pierwszego komputera, ktory tym kluczem poprawnie sie uwierzytelni. Recznie
przepisywana nazwa node'a byla zrodlem cichej awarii: nazwa inna niz ta, ktorej
uzywa Deskmate, konczyla sie zamknietym polaczeniem bez zadnego komunikatu.

Rekonfiguracja pozwala wygenerowac nowy klucz albo odpiac wpis od node'a
(np. po zmianie nazwy komputera) bez kasowania urzadzenia i jego encji.
"""
from __future__ import annotations

from typing import Any

import voluptuous as vol

from homeassistant.config_entries import ConfigEntry, ConfigFlow, ConfigFlowResult

from .const import CONF_CASCADE_KEY, CONF_KEY, CONF_NODE_ID, DOMAIN, UNBOUND_TITLE
from .crypto import gen_psk


class DeskmateLinkConfigFlow(ConfigFlow, domain=DOMAIN):
    VERSION = 1

    def __init__(self) -> None:
        self._key: str | None = None
        self._cascade: str | None = None

    # ── nowe parowanie ───────────────────────────────────────────

    async def async_step_user(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        if user_input is not None:
            self._key = gen_psk()
            return await self.async_step_show_key()
        return self.async_show_form(step_id="user", data_schema=vol.Schema({}))

    async def async_step_show_key(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        """Pokazuje wygenerowany klucz - jedyny raz, do wklejenia w Deskmate."""
        assert self._key is not None
        if user_input is not None:
            return self.async_create_entry(
                title=UNBOUND_TITLE,
                data={CONF_NODE_ID: "", CONF_KEY: self._key, CONF_CASCADE_KEY: ""},
            )
        return self.async_show_form(
            step_id="show_key",
            data_schema=vol.Schema({}),
            description_placeholders={"key": self._key},
        )

    # ── rekonfiguracja istniejacego wpisu ────────────────────────

    async def async_step_reconfigure(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        entry = self._reconfigure_entry()
        options = ["new_key", "unbind"]
        options.append(
            "cascade_off" if entry.data.get(CONF_CASCADE_KEY) else "cascade_on"
        )
        return self.async_show_menu(step_id="reconfigure", menu_options=options)

    async def async_step_cascade_on(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        """Wlacza druga warstwe szyfrowania (ChaCha20-Poly1305 nad AES-256-GCM)."""
        entry = self._reconfigure_entry()
        if self._cascade is None:
            self._cascade = gen_psk()
        if user_input is not None:
            return await self._update_and_abort(
                entry,
                {**entry.data, CONF_CASCADE_KEY: self._cascade},
                entry.title,
                entry.unique_id,
            )
        return self.async_show_form(
            step_id="cascade_on",
            data_schema=vol.Schema({}),
            description_placeholders={"key": self._cascade},
        )

    async def async_step_cascade_off(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        entry = self._reconfigure_entry()
        if user_input is not None:
            return await self._update_and_abort(
                entry,
                {**entry.data, CONF_CASCADE_KEY: ""},
                entry.title,
                entry.unique_id,
            )
        return self.async_show_form(step_id="cascade_off", data_schema=vol.Schema({}))

    async def async_step_new_key(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        """Rotacja klucza parowania - pokazany raz, do wklejenia w Deskmate."""
        entry = self._reconfigure_entry()
        if self._key is None:
            self._key = gen_psk()
        if user_input is not None:
            return await self._update_and_abort(
                entry,
                {**entry.data, CONF_KEY: self._key},
                entry.title,
                entry.unique_id,
            )
        return self.async_show_form(
            step_id="new_key",
            data_schema=vol.Schema({}),
            description_placeholders={"key": self._key},
        )

    async def async_step_unbind(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        """Odpina wpis od node'a - przypnie sie do nastepnego, ktory sie zglosi."""
        entry = self._reconfigure_entry()
        if user_input is not None:
            return await self._update_and_abort(
                entry, {**entry.data, CONF_NODE_ID: ""}, UNBOUND_TITLE, None
            )
        return self.async_show_form(
            step_id="unbind",
            data_schema=vol.Schema({}),
            description_placeholders={"node_id": entry.data.get(CONF_NODE_ID) or "-"},
        )

    # ── pomocnicze ───────────────────────────────────────────────

    def _reconfigure_entry(self) -> ConfigEntry:
        entry = self.hass.config_entries.async_get_entry(self.context["entry_id"])
        assert entry is not None
        return entry

    async def _update_and_abort(
        self,
        entry: ConfigEntry,
        data: dict[str, Any],
        title: str,
        unique_id: str | None,
    ) -> ConfigFlowResult:
        self.hass.config_entries.async_update_entry(
            entry, data=data, title=title, unique_id=unique_id
        )
        await self.hass.config_entries.async_reload(entry.entry_id)
        return self.async_abort(reason="reconfigure_successful")
