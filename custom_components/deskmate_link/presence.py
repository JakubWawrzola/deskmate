"""Whether a computer counts as connected, and which entities it serves.

The hub reports what happens to its session. Entities read the result
through `serves`, and the Connection sensor through `connection`
(docs/LINK.md, "Session frames").

A session changes nothing by authenticating. The computer becomes available
with the session's first `declare`, because only that frame says which
entities the client serves: making everything available first would show
entities that a machine-level client is about to take away again.
"""

from __future__ import annotations

from collections.abc import Callable
from datetime import datetime
from typing import Any

from homeassistant.const import EVENT_HOMEASSISTANT_STOP
from homeassistant.core import CALLBACK_TYPE, Event, HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_send
from homeassistant.helpers.event import async_call_later

from .const import (
    BYE_HANDOVER,
    BYE_SHUTDOWN,
    BYE_SUSPEND,
    CONNECTION_CONNECTED,
    CONNECTION_DISCONNECTED,
    CONNECTION_OFF,
    CONNECTION_SLEEPING,
    DECLARE_WAIT_S,
    HANDOVER_WINDOW_S,
    SIGNAL_AVAILABLE,
    SIGNAL_CONNECTION,
)

_CONNECTION_AFTER_BYE = {
    BYE_SUSPEND: CONNECTION_SLEEPING,
    BYE_SHUTDOWN: CONNECTION_OFF,
}


def connection_unique_id(entry_id: str) -> str:
    """Registry id of the Connection sensor of a config entry.

    An entity that the client declares is `<entry id>-<key>`. This id has a
    colon where those have the hyphen, so no declared key can produce it.
    """
    return f"{entry_id}:connection"


class Presence:
    """The availability of one computer and of its entities."""

    def __init__(self, hass: HomeAssistant, entry_id: str) -> None:
        self._hass = hass
        self._entry_id = entry_id
        self.available = False
        # Keys that the machine session serves. None when nothing is
        # filtered: after a full declaration, and without a session.
        self.active_keys: frozenset[str] | None = None
        # The state of the Connection sensor.
        self.connection: str = CONNECTION_DISCONNECTED
        # The reason of the `bye` that ended the last session, until the next
        # session starts. None when that session said nothing.
        self.last_bye: str | None = None
        # The `bye` of the running session. It counts once the socket closes.
        self._bye: str | None = None
        # The role from the `info` of the running session.
        self._role: str | None = None
        # True from the handshake until the session has made the computer
        # available, with its first `declare` or after DECLARE_WAIT_S.
        self._awaiting_declare = False
        # One timer at a time: the wait for a `declare` while a session is
        # open, the handover window while none is.
        self._cancel_timer: CALLBACK_TYPE | None = None
        self._cancel_stop_listener: CALLBACK_TYPE | None = None

    def serves(self, key: str) -> bool:
        """Whether the entity `key` is available right now."""
        return self.available and not self.limits_out(key)

    def limits_out(self, key: str) -> bool:
        """Whether a machine declaration keeps the entity `key` out."""
        return self.active_keys is not None and key not in self.active_keys

    def only_served(self, states: dict[str, Any]) -> dict[str, Any]:
        """The part of a `state` frame that the session may update."""
        if self.active_keys is None:
            return states
        return {key: value for key, value in states.items() if key in self.active_keys}

    @callback
    def session_started(self) -> None:
        """A client authenticated. Nothing shows until it declares."""
        self._stop_timer()
        self._bye = None
        self.last_bye = None
        self._role = None
        self._awaiting_declare = True
        if not self._hass.is_stopping:
            self._start_timer(DECLARE_WAIT_S, self._declare_wait_over)

    @callback
    def client_identified(self, role: str) -> None:
        """The client of the session sent a valid `info`."""
        self._role = role
        self.connection = role
        self._notify_connection()

    @callback
    def bye_received(self, reason: str) -> None:
        """The client is about to close the socket, for this reason."""
        self._bye = reason

    @callback
    def session_ended(self) -> None:
        self._stop_timer()
        self._awaiting_declare = False
        self.last_bye, self._bye = self._bye, None
        if self.last_bye == BYE_HANDOVER and not self._hass.is_stopping:
            # The computer's other client connects in a moment. Until then
            # everything stays as the closed session left it.
            self._start_timer(HANDOVER_WINDOW_S, self._handover_expired)
            return
        self._go_offline()

    @callback
    def serve_only(self, keys: frozenset[str]) -> None:
        """A machine declaration: every other entity is unavailable.

        The limit and the availability change together, so an entity outside
        the keys is not available for a moment in between.
        """
        self.active_keys = keys
        self._go_online()

    @callback
    def serve_all(self) -> None:
        """A full declaration: nothing is filtered."""
        if not self._awaiting_declare and self.active_keys is None:
            return
        self.active_keys = None
        self._go_online()

    @callback
    def async_shutdown(self) -> None:
        """The entry is unloading: leave no timer behind."""
        self._stop_timer()

    def _go_online(self) -> None:
        self._stop_timer()
        self._awaiting_declare = False
        self.available = True
        self.connection = self._role or CONNECTION_CONNECTED
        self._notify_entities()
        self._notify_connection()

    def _go_offline(self) -> None:
        self.active_keys = None
        self.available = False
        self.connection = CONNECTION_DISCONNECTED
        if self.last_bye is not None:
            self.connection = _CONNECTION_AFTER_BYE.get(
                self.last_bye, CONNECTION_DISCONNECTED
            )
        self._notify_entities()
        self._notify_connection()

    def _start_timer(self, delay: float, action: Callable[[datetime], None]) -> None:
        self._cancel_timer = async_call_later(self._hass, delay, action)
        self._cancel_stop_listener = self._hass.bus.async_listen_once(
            EVENT_HOMEASSISTANT_STOP, self._hass_stopping
        )

    def _stop_timer(self) -> None:
        if self._cancel_timer is not None:
            self._cancel_timer()
            self._cancel_timer = None
        if self._cancel_stop_listener is not None:
            self._cancel_stop_listener()
            self._cancel_stop_listener = None

    @callback
    def _declare_wait_over(self, _now: datetime) -> None:
        # The client declares nothing, so there is no limit to apply.
        self.active_keys = None
        self._go_online()

    @callback
    def _handover_expired(self, _now: datetime) -> None:
        self._stop_timer()
        self._go_offline()

    @callback
    def _hass_stopping(self, _event: Event) -> None:
        # A one-time listener has removed itself by the time it runs.
        self._cancel_stop_listener = None
        self._stop_timer()
        if not self._awaiting_declare:
            # The handover window: nobody takes over any more.
            self._go_offline()

    def _notify_entities(self) -> None:
        async_dispatcher_send(self._hass, f"{SIGNAL_AVAILABLE}_{self._entry_id}")

    def _notify_connection(self) -> None:
        async_dispatcher_send(self._hass, f"{SIGNAL_CONNECTION}_{self._entry_id}")
