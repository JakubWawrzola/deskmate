"""Whether a computer counts as connected, and which entities it serves.

The hub reports what happens to its session. Entities read the result
through `serves` (docs/LINK.md, "Session frames").
"""

from __future__ import annotations

from datetime import datetime
from typing import Any

from homeassistant.const import EVENT_HOMEASSISTANT_STOP
from homeassistant.core import CALLBACK_TYPE, Event, HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_send
from homeassistant.helpers.event import async_call_later

from .const import BYE_HANDOVER, HANDOVER_WINDOW_S, SIGNAL_AVAILABLE


class Presence:
    """The availability of one computer and of its entities."""

    def __init__(self, hass: HomeAssistant, entry_id: str) -> None:
        self._hass = hass
        self._entry_id = entry_id
        self.available = False
        # Keys that the connected machine session serves. None when nothing
        # is filtered: after a full declaration, and without a session.
        self.active_keys: frozenset[str] | None = None
        # The reason of the `bye` that ended the last session, until the next
        # session starts. None when that session said nothing.
        self.last_bye: str | None = None
        # The `bye` of the running session. It counts once the socket closes.
        self._bye: str | None = None
        self._cancel_handover_timer: CALLBACK_TYPE | None = None
        self._cancel_stop_listener: CALLBACK_TYPE | None = None

    def serves(self, key: str) -> bool:
        """Whether the entity `key` is available right now."""
        return self.available and (self.active_keys is None or key in self.active_keys)

    def only_served(self, states: dict[str, Any]) -> dict[str, Any]:
        """The part of a `state` frame that the session may update."""
        if self.active_keys is None:
            return states
        return {key: value for key, value in states.items() if key in self.active_keys}

    @callback
    def session_started(self) -> None:
        self._close_handover_window()
        self._bye = None
        self.last_bye = None
        self.active_keys = None
        self._set_available(True)

    @callback
    def bye_received(self, reason: str) -> None:
        """The client is about to close the socket, for this reason."""
        self._bye = reason

    @callback
    def session_ended(self) -> None:
        self.last_bye, self._bye = self._bye, None
        if self.last_bye == BYE_HANDOVER and not self._hass.is_stopping:
            # The computer's other client connects in a moment. Until then
            # everything stays as the closed session left it.
            self._open_handover_window()
            return
        self._go_offline()

    @callback
    def serve_only(self, keys: frozenset[str]) -> None:
        """A machine declaration: every other entity becomes unavailable."""
        self.active_keys = keys
        self._notify_entities()

    @callback
    def serve_all(self) -> None:
        """A full declaration: nothing is filtered any more."""
        if self.active_keys is None:
            return
        self.active_keys = None
        self._notify_entities()

    @callback
    def async_shutdown(self) -> None:
        """The entry is unloading: leave no timer behind."""
        self._close_handover_window()

    def _go_offline(self) -> None:
        self.active_keys = None
        self._set_available(False)

    def _open_handover_window(self) -> None:
        self._cancel_handover_timer = async_call_later(
            self._hass, HANDOVER_WINDOW_S, self._handover_expired
        )
        self._cancel_stop_listener = self._hass.bus.async_listen_once(
            EVENT_HOMEASSISTANT_STOP, self._hass_stopping
        )

    def _close_handover_window(self) -> None:
        if self._cancel_handover_timer is not None:
            self._cancel_handover_timer()
            self._cancel_handover_timer = None
        if self._cancel_stop_listener is not None:
            self._cancel_stop_listener()
            self._cancel_stop_listener = None

    @callback
    def _handover_expired(self, _now: datetime) -> None:
        self._close_handover_window()
        self._go_offline()

    @callback
    def _hass_stopping(self, _event: Event) -> None:
        # A one-time listener has removed itself by the time it runs.
        self._cancel_stop_listener = None
        self._close_handover_window()
        self._go_offline()

    def _set_available(self, value: bool) -> None:
        self.available = value
        self._notify_entities()

    def _notify_entities(self) -> None:
        async_dispatcher_send(self._hass, f"{SIGNAL_AVAILABLE}_{self._entry_id}")
