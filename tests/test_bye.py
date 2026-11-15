"""The `bye` session frame: why a client closes the socket, and what follows."""

from __future__ import annotations

import logging
from collections.abc import Awaitable, Callable
from datetime import timedelta
from typing import Any

import pytest
from homeassistant.const import EVENT_HOMEASSISTANT_STOP, STATE_UNAVAILABLE
from homeassistant.core import CoreState, Event, HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from homeassistant.helpers.event import async_track_state_change_event
from homeassistant.util import dt as dt_util
from pytest_homeassistant_custom_component.common import (
    MockConfigEntry,
    async_fire_time_changed,
)

from custom_components.deskmate_link.bye import invalid_bye_field
from custom_components.deskmate_link.const import (
    HANDOVER_WINDOW_S,
    SIGNAL_AVAILABLE,
)
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import NODE_ID, Connect, LinkClient
from .test_declare_scope import declare_machine, state_of
from .test_session import CPU_SENSOR, KEEP_AWAKE_SWITCH, declare, entity_id

HUB_LOGGER = "custom_components.deskmate_link.hub"
Wait = Callable[[], Awaitable[None]]


async def say_bye_and_close(client: LinkClient, reason: str) -> None:
    await client.send({"t": "bye", "reason": reason})
    await client.close()


async def pass_time(hass: HomeAssistant, seconds: float) -> None:
    """Run the timers that are due within this many seconds."""
    async_fire_time_changed(hass, dt_util.utcnow() + timedelta(seconds=seconds))
    await hass.async_block_till_done()


def count_availability_signals(hass: HomeAssistant, hub: DeskmateHub) -> list[bool]:
    """Collect what the hub reported each time it told its entities to look."""
    seen: list[bool] = []

    @callback
    def _record() -> None:
        seen.append(hub.available)

    async_dispatcher_connect(hass, f"{SIGNAL_AVAILABLE}_{hub.entry.entry_id}", _record)
    return seen


# ── validation ───────────────────────────────────────────────────


@pytest.mark.parametrize("reason", ["handover", "suspend", "shutdown"])
def test_valid_bye_frame_is_accepted(reason: str) -> None:
    assert invalid_bye_field({"t": "bye", "reason": reason, "later": "x"}) is None


@pytest.mark.parametrize(
    "reason",
    ["Handover", " handover", "handover\n", "sleep", "", None, 1, True, ["handover"]],
)
def test_invalid_bye_reason_is_named(reason: Any) -> None:
    assert invalid_bye_field({"t": "bye", "reason": reason}) == "reason"


def test_missing_bye_reason_is_named() -> None:
    assert invalid_bye_field({"t": "bye"}) == "reason"


# ── handover ─────────────────────────────────────────────────────


async def test_handover_keeps_the_computer_available_until_the_window_ends(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    await client.send({"t": "state", "s": {"cpu_usage": 42}})

    await say_bye_and_close(client, "handover")
    await wait_until_session_ended()

    assert hub.available
    assert state_of(hass, entry, "sensor", "cpu_usage") == "42"
    await pass_time(hass, HANDOVER_WINDOW_S - 5)
    assert hub.available
    await pass_time(hass, HANDOVER_WINDOW_S + 1)
    assert not hub.available
    assert state_of(hass, entry, "sensor", "cpu_usage") == STATE_UNAVAILABLE


async def test_session_inside_the_window_takes_over_without_a_gap(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR])
    await first.send({"t": "state", "s": {"cpu_usage": 42}})
    await first.sync()
    states: list[str] = []

    @callback
    def _record(event: Event) -> None:
        states.append(event.data["new_state"].state)

    async_track_state_change_event(
        hass, [entity_id(hass, entry, "sensor", "cpu_usage")], _record
    )

    await say_bye_and_close(first, "handover")
    await wait_until_session_ended()
    second = await connect()
    await declare(hass, second, [CPU_SENSOR])
    await second.send({"t": "state", "s": {"cpu_usage": 43}})
    await second.sync()
    # The window of the first session must not end the second one's presence.
    await pass_time(hass, HANDOVER_WINDOW_S + 1)

    assert hub.available
    assert states == ["43"]


async def test_commands_in_the_handover_gap_fail_as_not_connected(
    hass: HomeAssistant,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await declare_machine(hass, client, [CPU_SENSOR])

    await say_bye_and_close(client, "handover")
    await wait_until_session_ended()

    # Also for a key that the closed machine session did not serve.
    for key in ("cpu_usage", "keep_awake"):
        with pytest.raises(ConnectionError, match="deskmate not connected"):
            await hub.async_command(key, "set", True)


async def test_handover_after_a_machine_session_keeps_its_limit(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await first.send({"t": "state", "s": {"cpu_usage": 42, "keep_awake": True}})
    await declare_machine(hass, first, [CPU_SENSOR])

    await say_bye_and_close(first, "handover")
    await wait_until_session_ended()

    assert state_of(hass, entry, "sensor", "cpu_usage") == "42"
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE
    # The session that takes over changes nothing before it declares.
    second = await connect()
    await second.sync()
    assert hub.presence.active_keys == {"cpu_usage"}
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE
    await declare(hass, second, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    assert hub.presence.active_keys is None
    assert state_of(hass, entry, "switch", "keep_awake") == "on"


async def test_end_of_the_window_lifts_the_limit_of_the_machine_session(
    hass: HomeAssistant,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await declare_machine(hass, client, [CPU_SENSOR])
    await say_bye_and_close(client, "handover")
    await wait_until_session_ended()
    assert hub.presence.active_keys == {"cpu_usage"}

    await pass_time(hass, HANDOVER_WINDOW_S + 1)

    assert not hub.available
    assert hub.presence.active_keys is None


# ── suspend and shutdown ─────────────────────────────────────────


@pytest.mark.parametrize("reason", ["suspend", "shutdown"])
async def test_suspend_and_shutdown_end_availability_at_once(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
    reason: str,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])

    await say_bye_and_close(client, reason)
    await wait_until_session_ended()

    assert not hub.available
    assert state_of(hass, entry, "sensor", "cpu_usage") == STATE_UNAVAILABLE
    assert hub.presence.last_bye == reason


async def test_next_session_forgets_the_last_bye(
    hub: DeskmateHub, connect: Connect, wait_until_session_ended: Wait
) -> None:
    first = await connect()
    await say_bye_and_close(first, "suspend")
    await wait_until_session_ended()

    second = await connect()
    await second.sync()

    assert hub.presence.last_bye is None


async def test_bye_of_a_replaced_session_does_not_count_for_the_next_one(
    hub: DeskmateHub, connect: Connect, wait_until_offline: Wait
) -> None:
    first = await connect()
    await first.send({"t": "bye", "reason": "handover"})
    await first.sync()
    second = await connect()
    await first.wait_closed()

    await second.close()
    await wait_until_offline()

    assert hub.presence.last_bye is None


# ── invalid frames ───────────────────────────────────────────────


@pytest.mark.parametrize(
    "frame",
    [
        {"t": "bye"},
        {"t": "bye", "reason": "reason-RECEIVED"},
        {"t": "bye", "reason": "HANDOVER-RECEIVED"},
        {"t": "bye", "reason": ["handover", "RECEIVED"]},
        {"t": "bye", "reason": None},
    ],
)
async def test_invalid_bye_is_ignored_and_logged_without_its_value(
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Wait,
    caplog: pytest.LogCaptureFixture,
    frame: dict[str, Any],
) -> None:
    client = await connect()

    await client.send({**frame, "extra": "extra-RECEIVED"})
    # sync() fails if the hub answers the frame.
    await client.sync()
    await client.close()
    await wait_until_offline()

    assert hub.presence.last_bye is None
    warnings = [
        record.getMessage()
        for record in caplog.records
        if record.name == HUB_LOGGER and record.levelno == logging.WARNING
    ]
    assert warnings == [
        f"deskmate_link[{NODE_ID}]: ignoring a bye frame with an invalid reason"
    ]
    assert "RECEIVED" not in caplog.text


async def test_valid_bye_gets_no_answer(connect: Connect) -> None:
    client = await connect()

    await client.send({"t": "bye", "reason": "suspend"})

    # sync() fails if the first frame from the hub is anything but its `pong`.
    await client.sync()


# ── the window and the life of Home Assistant ────────────────────


async def test_stopping_home_assistant_closes_the_handover_window(
    hass: HomeAssistant,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    await say_bye_and_close(client, "handover")
    await wait_until_session_ended()
    signals = count_availability_signals(hass, hub)

    hass.bus.async_fire(EVENT_HOMEASSISTANT_STOP)
    await hass.async_block_till_done()

    assert signals == [False]
    # The timer is gone: nothing more happens when its time comes.
    await pass_time(hass, HANDOVER_WINDOW_S + 1)
    assert signals == [False]


async def test_unloading_the_entry_closes_the_handover_window(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    await say_bye_and_close(client, "handover")
    await wait_until_session_ended()
    signals = count_availability_signals(hass, hub)

    assert await hass.config_entries.async_unload(entry.entry_id)
    await pass_time(hass, HANDOVER_WINDOW_S + 1)

    assert signals == []


async def test_no_handover_window_opens_while_home_assistant_stops(
    hass: HomeAssistant,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    assert hub.available
    await client.send({"t": "bye", "reason": "handover"})
    await client.sync()
    hass.set_state(CoreState.stopping)

    await client.close()
    await wait_until_session_ended()

    assert not hub.available
