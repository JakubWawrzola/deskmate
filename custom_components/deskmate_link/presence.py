"""Whether a computer counts as connected, and which entities it serves.

The hub reports what happens to its session. Entities read the result
through `serves` (docs/LINK.md, "Session frames").
"""

from __future__ import annotations

from typing import Any

from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_send

from .const import SIGNAL_AVAILABLE


class Presence:
    """The availability of one computer and of its entities."""

    def __init__(self, hass: HomeAssistant, entry_id: str) -> None:
        self._hass = hass
        self._entry_id = entry_id
        self.available = False
        # Keys that the connected machine session serves. None when nothing
        # is filtered: after a full declaration, and without a session.
        self.active_keys: frozenset[str] | None = None

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
        self.active_keys = None
        self._set_available(True)

    @callback
    def session_ended(self) -> None:
        self.active_keys = None
        self._set_available(False)

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

    def _set_available(self, value: bool) -> None:
        self.available = value
        self._notify_entities()

    def _notify_entities(self) -> None:
        async_dispatcher_send(self._hass, f"{SIGNAL_AVAILABLE}_{self._entry_id}")
