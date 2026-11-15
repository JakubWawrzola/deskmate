"""When a computer becomes available: with the first `declare` of a session."""

from __future__ import annotations

from collections.abc import Awaitable, Callable

from homeassistant.const import (
    EVENT_HOMEASSISTANT_STOP,
    STATE_OFF,
    STATE_ON,
    STATE_UNAVAILABLE,
)
from homeassistant.core import CoreState, Event, HomeAssistant, callback
from homeassistant.helpers.event import async_track_state_change_event
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import DECLARE_WAIT_S
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import Connect, LinkClient
from .test_bye import count_availability_signals, pass_time
from .test_declare_scope import LINK_SENSOR, declare_machine, state_of
from .test_session import (
    CPU_SENSOR,
    DEVICE,
    KEEP_AWAKE_SWITCH,
    declare,
    entity_id,
)

SERVICE_INFO = {"t": "info", "role": "service", "platform": "windows", "app": "0.9.0"}
Wait = Callable[[], Awaitable[None]]


async def app_was_here(
    hass: HomeAssistant, connect: Connect, wait_until_offline: Wait
) -> None:
    """Leave the stored state of an app session that ended without `bye`."""
    app = await connect()
    await declare(hass, app, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await app.send({"t": "state", "s": {"cpu_usage": 10, "keep_awake": True}})
    await app.sync()
    await app.close()
    await wait_until_offline()


def record_states(hass: HomeAssistant, entity: str | None) -> list[str]:
    """Collect every state that the entity takes from now on."""
    assert entity is not None
    states: list[str] = []

    @callback
    def _record(event: Event) -> None:
        states.append(event.data["new_state"].state)

    async_track_state_change_event(hass, [entity], _record)
    return states


async def start_service_session(connect: Connect) -> LinkClient:
    """Open a session and say that it is the Windows service."""
    client = await connect()
    await client.send(SERVICE_INFO)
    assert (await client.receive())["t"] == "info"
    return client


# ── clients that declare, as every released one does ─────────────


async def test_handshake_alone_does_not_make_the_computer_available(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Wait,
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR, LINK_SENSOR])
    await first.close()
    await wait_until_offline()

    second = await connect()
    await second.sync()

    assert hub.connected
    assert not hub.available
    assert state_of(hass, entry, "sensor", "cpu_usage") == STATE_UNAVAILABLE
    assert state_of(hass, entry, "binary_sensor", "link") == STATE_OFF


async def test_old_client_is_available_as_soon_as_its_declare_is_handled(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Wait,
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR, LINK_SENSOR])
    await first.send({"t": "state", "s": {"cpu_usage": 42}})
    await first.sync()
    await first.close()
    await wait_until_offline()
    second = await connect()

    # Deskmate up to 0.8.5 sends `declare` first and no `info`.
    await second.send(
        {"t": "declare", "device": DEVICE, "entities": [CPU_SENSOR, LINK_SENSOR]}
    )
    # The `pong` says that the hub has handled the declare, and nothing more.
    await second.sync()

    assert hub.available
    assert hub.presence.active_keys is None
    assert state_of(hass, entry, "sensor", "cpu_usage") == "42"
    assert state_of(hass, entry, "binary_sensor", "link") == STATE_ON


async def test_first_declare_of_a_new_computer_makes_it_available(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.sync()
    assert not hub.available

    await declare(hass, client, [CPU_SENSOR])

    assert hub.available
    assert state_of(hass, entry, "sensor", "cpu_usage") != STATE_UNAVAILABLE


async def test_command_before_the_declare_is_still_sent(
    hass: HomeAssistant, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.sync()

    press = hass.async_create_task(hub.async_command("lock", "press"))
    command = await client.receive()
    await client.send({"t": "ack", "id": command["id"], "ok": True})
    await press

    assert (command["key"], command["action"]) == ("lock", "press")


# ── the Windows service after a plain disconnect ─────────────────


async def test_service_never_makes_a_user_level_entity_available(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Wait,
) -> None:
    await app_was_here(hass, connect, wait_until_offline)
    user_level = record_states(hass, entity_id(hass, entry, "switch", "keep_awake"))

    service = await start_service_session(connect)
    await declare_machine(hass, service, [CPU_SENSOR])
    await service.send({"t": "state", "s": {"cpu_usage": 20}})
    await service.sync()
    # The wait for a declare ended with the declare: it lifts nothing later.
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert user_level == []
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE
    assert state_of(hass, entry, "sensor", "cpu_usage") == "20"
    assert hub.presence.active_keys == {"cpu_usage"}


async def test_service_that_replaces_a_silent_session_keeps_user_level_entities_out(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Wait,
) -> None:
    await app_was_here(hass, connect, wait_until_offline)
    user_level = record_states(hass, entity_id(hass, entry, "switch", "keep_awake"))
    silent = await connect()
    await silent.sync()

    service = await start_service_session(connect)
    await silent.wait_closed()
    await declare_machine(hass, service, [CPU_SENSOR])
    # Neither the replaced session's wait nor this one's may end now.
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert user_level == []
    assert hub.available
    assert hub.presence.active_keys == {"cpu_usage"}


async def test_session_that_replaces_another_keeps_what_was_reported(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await declare_machine(hass, first, [CPU_SENSOR])

    second = await connect()
    await first.wait_closed()
    await second.sync()

    assert hub.available
    assert hub.presence.active_keys == {"cpu_usage"}
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE


# ── a client that never declares ─────────────────────────────────


async def test_client_that_never_declares_becomes_available_after_the_wait(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Wait,
) -> None:
    await app_was_here(hass, connect, wait_until_offline)
    client = await connect()
    await client.sync()

    await pass_time(hass, 0.5)
    assert not hub.available
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert hub.available
    assert hub.presence.active_keys is None
    assert state_of(hass, entry, "sensor", "cpu_usage") == "10"
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_ON


async def test_wait_for_a_declare_ends_with_the_session(
    hass: HomeAssistant, hub: DeskmateHub, connect: Connect, wait_until_offline: Wait
) -> None:
    client = await connect()
    await client.sync()
    signals = count_availability_signals(hass, hub)

    await client.close()
    await wait_until_offline()
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert signals == [False]


async def test_wait_for_a_declare_ends_when_the_entry_unloads(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.sync()
    signals = count_availability_signals(hass, hub)

    assert await hass.config_entries.async_unload(entry.entry_id)
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert signals == []


async def test_wait_for_a_declare_ends_when_home_assistant_stops(
    hass: HomeAssistant, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.sync()
    signals = count_availability_signals(hass, hub)

    hass.bus.async_fire(EVENT_HOMEASSISTANT_STOP)
    await hass.async_block_till_done()
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert True not in signals
    assert not hub.available


async def test_no_wait_for_a_declare_starts_while_home_assistant_stops(
    hass: HomeAssistant, hub: DeskmateHub
) -> None:
    hass.set_state(CoreState.stopping)

    # Home Assistant answers 503 to a new connection while it stops, so the
    # hub is told about the session directly.
    hub.presence.session_started()
    await pass_time(hass, DECLARE_WAIT_S + 1)

    assert not hub.available
    # A declare still makes the computer available.
    hub.presence.serve_all()
    assert hub.available
