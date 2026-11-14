"""The Connection sensor: which client is connected, or why none is."""

from __future__ import annotations

import json
from collections.abc import Awaitable, Callable
from datetime import timedelta
from pathlib import Path
from typing import Any

import pytest
from homeassistant.const import EVENT_HOMEASSISTANT_FINAL_WRITE, EntityCategory
from homeassistant.core import HomeAssistant
from homeassistant.helpers import entity_registry as er
from homeassistant.util import dt as dt_util
from pytest_homeassistant_custom_component.common import (
    MockConfigEntry,
    async_fire_time_changed,
)

from custom_components.deskmate_link.const import (
    CONNECTION_STATES,
    DOMAIN,
    HANDOVER_WINDOW_S,
    INFO_ROLES,
)
from custom_components.deskmate_link.presence import connection_unique_id

from .link_client import Connect, LinkClient
from .test_declare_scope import declare_machine
from .test_session import CPU_SENSOR, LOCK_BUTTON, declare, entity_id

INTEGRATION = Path(__file__).parents[1] / "custom_components/deskmate_link"
INFO = {"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
Wait = Callable[[], Awaitable[None]]


def connection_entity(hass: HomeAssistant, entry: MockConfigEntry) -> str | None:
    """The entity id of the entry's Connection sensor, if it exists."""
    return er.async_get(hass).async_get_entity_id(
        "sensor", DOMAIN, connection_unique_id(entry.entry_id)
    )


def connection(hass: HomeAssistant, entry: MockConfigEntry) -> str:
    """The state of the entry's Connection sensor."""
    sensor = connection_entity(hass, entry)
    assert sensor is not None
    return hass.states.get(sensor).state


async def identify(client: LinkClient, role: str) -> None:
    """Send `info` with this role and wait for the hub's answer."""
    await client.send({**INFO, "role": role})
    assert (await client.receive())["t"] == "info"


async def paired_computer(hass: HomeAssistant, connect: Connect) -> LinkClient:
    """A session of a client that has declared, so the sensor exists."""
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    return client


# ── the entity ───────────────────────────────────────────────────


async def test_sensor_is_a_diagnostic_enum_on_the_device_of_the_computer(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    await paired_computer(hass, connect)

    registry = er.async_get(hass)
    sensor = registry.async_get(connection_entity(hass, entry))
    declared = registry.async_get(entity_id(hass, entry, "sensor", "cpu_usage"))
    assert sensor.entity_category is EntityCategory.DIAGNOSTIC
    assert sensor.device_id == declared.device_id
    assert sensor.translation_key == "connection"
    state = hass.states.get(sensor.entity_id)
    assert state.attributes["device_class"] == "enum"
    assert state.attributes["options"] == list(CONNECTION_STATES)
    assert state.attributes["friendly_name"] == "Test PC Connection"


def test_every_role_is_a_state_of_the_sensor() -> None:
    assert set(CONNECTION_STATES) == INFO_ROLES | {
        "connected",
        "sleeping",
        "off",
        "disconnected",
    }


def paths(node: dict[str, Any], prefix: str = "") -> set[str]:
    """The path of every text in a translation file."""
    found: set[str] = set()
    for key, value in node.items():
        if isinstance(value, dict):
            found |= paths(value, f"{prefix}{key}.")
        else:
            found.add(f"{prefix}{key}")
    return found


def test_translations_change_together() -> None:
    strings = json.loads((INTEGRATION / "strings.json").read_text(encoding="utf-8"))
    english = json.loads(
        (INTEGRATION / "translations/en.json").read_text(encoding="utf-8")
    )
    polish = json.loads(
        (INTEGRATION / "translations/pl.json").read_text(encoding="utf-8")
    )

    assert english == strings
    assert paths(polish) == paths(strings)
    for translation in (strings, polish):
        sensor = translation["entity"]["sensor"]["connection"]
        assert sensor["name"]
        assert set(sensor["state"]) == set(CONNECTION_STATES)


async def test_entry_that_waits_for_pairing_has_no_sensor_yet(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    assert connection_entity(hass, entry) is None

    await paired_computer(hass, connect)

    assert connection(hass, entry) == "connected"


async def test_machine_declaration_alone_creates_the_sensor(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await identify(client, "service")

    await declare_machine(hass, client, [CPU_SENSOR])

    assert connection(hass, entry) == "service"


async def test_sensor_exists_after_a_restart_before_the_computer_connects(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    await paired_computer(hass, connect)
    sensor = connection_entity(hass, entry)
    hass.bus.async_fire(EVENT_HOMEASSISTANT_FINAL_WRITE)
    await hass.async_block_till_done()

    assert await hass.config_entries.async_reload(entry.entry_id)
    await hass.async_block_till_done()

    assert connection_entity(hass, entry) == sensor
    assert connection(hass, entry) == "disconnected"


async def test_declare_never_removes_the_sensor(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await paired_computer(hass, connect)
    sensor = connection_entity(hass, entry)

    await declare(hass, client, [])

    assert entity_id(hass, entry, "sensor", "cpu_usage") is None
    assert connection_entity(hass, entry) == sensor
    assert connection(hass, entry) == "connected"


async def test_declared_key_cannot_take_the_place_of_the_sensor(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    lookalikes = [
        {"key": "connection", "kind": "sensor", "name": "Declared connection"},
        {"key": ":connection", "kind": "sensor", "name": "Declared :connection"},
    ]

    await declare(hass, client, lookalikes)
    await client.send({"t": "state", "s": {"connection": "x", ":connection": "y"}})
    await client.sync()

    registered = {
        connection_entity(hass, entry),
        entity_id(hass, entry, "sensor", "connection"),
        entity_id(hass, entry, "sensor", ":connection"),
    }
    assert None not in registered
    assert len(registered) == 3
    assert connection(hass, entry) == "connected"


# ── states ───────────────────────────────────────────────────────


async def test_client_without_info_is_connected_and_then_disconnected(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    wait_until_offline: Wait,
) -> None:
    client = await paired_computer(hass, connect)
    assert connection(hass, entry) == "connected"

    await client.close()
    await wait_until_offline()

    # Always available: the end of the session is a state, not a gap.
    assert connection(hass, entry) == "disconnected"


@pytest.mark.parametrize("role", ["app", "service", "daemon"])
async def test_role_from_info_is_the_state(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect, role: str
) -> None:
    client = await paired_computer(hass, connect)

    await identify(client, role)

    assert connection(hass, entry) == role


async def test_invalid_info_leaves_the_state_alone(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await paired_computer(hass, connect)

    await client.send({**INFO, "role": "admin"})
    await client.sync()

    assert connection(hass, entry) == "connected"


@pytest.mark.parametrize(
    ("reason", "state"), [("suspend", "sleeping"), ("shutdown", "off")]
)
async def test_bye_names_the_state_until_the_next_session(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    wait_until_offline: Wait,
    reason: str,
    state: str,
) -> None:
    first = await paired_computer(hass, connect)
    await identify(first, "app")

    await first.send({"t": "bye", "reason": reason})
    await first.sync()
    # The reason counts once the socket is closed.
    assert connection(hass, entry) == "app"
    await first.close()
    await wait_until_offline()

    assert connection(hass, entry) == state
    second = await connect()
    await second.sync()
    assert connection(hass, entry) == "connected"


async def test_handover_keeps_the_state_until_the_window_ends(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    client = await paired_computer(hass, connect)
    await identify(client, "app")

    await client.send({"t": "bye", "reason": "handover"})
    await client.close()
    await wait_until_session_ended()

    assert connection(hass, entry) == "app"
    async_fire_time_changed(
        hass, dt_util.utcnow() + timedelta(seconds=HANDOVER_WINDOW_S + 1)
    )
    await hass.async_block_till_done()
    assert connection(hass, entry) == "disconnected"


async def test_session_that_takes_over_shows_its_own_role(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    wait_until_session_ended: Wait,
) -> None:
    app = await paired_computer(hass, connect)
    await identify(app, "app")
    await app.send({"t": "bye", "reason": "handover"})
    await app.close()
    await wait_until_session_ended()

    service = await connect()
    await service.sync()
    # The new session does not inherit the role of the one before it.
    assert connection(hass, entry) == "connected"
    await identify(service, "service")

    assert connection(hass, entry) == "service"


# ── next to the entities that the client declares ────────────────


async def test_sensor_stays_available_during_a_machine_session(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, LOCK_BUTTON])
    await identify(client, "service")

    await declare_machine(hass, client, [CPU_SENSOR])

    assert connection(hass, entry) == "service"
