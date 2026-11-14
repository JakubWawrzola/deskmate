"""`declare` with the scope "machine": a client that serves part of the entities."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from typing import Any

import pytest
from homeassistant.const import (
    EVENT_HOMEASSISTANT_FINAL_WRITE,
    STATE_OFF,
    STATE_ON,
    STATE_UNAVAILABLE,
)
from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers import device_registry as dr
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link import entity as entity_module
from custom_components.deskmate_link.const import DOMAIN
from custom_components.deskmate_link.declare_scope import (
    NEEDS_APP_ERROR,
    declared_keys,
    is_machine_declare,
    merge_machine_declaration,
)
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import NODE_ID, Connect, LinkClient
from .test_session import (
    CPU_SENSOR,
    DEVICE,
    KEEP_AWAKE_SWITCH,
    LOCK_BUTTON,
    declare,
    entity_id,
)

SERVICE_DEVICE = {
    "name": "Named by the service",
    "model": "Deskmate service",
    "sw_version": "9.9.9",
}
UPTIME_SENSOR = {"key": "uptime", "kind": "sensor", "name": "Uptime"}
LINK_SENSOR = {
    "key": "link",
    "kind": "binary_sensor",
    "name": "Connection",
    "link": True,
}
NOT_MACHINE_SCOPES = [
    "full",
    "user",
    "Machine",
    "machine ",
    "",
    None,
    1,
    True,
    ["machine"],
    {"machine": True},
]


async def declare_machine(
    hass: HomeAssistant, client: LinkClient, entities: list[dict[str, Any]]
) -> None:
    """Send a machine declaration and wait until Home Assistant has applied it."""
    await client.send(
        {
            "t": "declare",
            "scope": "machine",
            "device": SERVICE_DEVICE,
            "entities": entities,
        }
    )
    await client.sync()
    await hass.async_block_till_done()


async def stored_declaration(
    hass: HomeAssistant, hass_storage: dict[str, Any], entry: MockConfigEntry
) -> dict[str, Any]:
    """What the hub has saved, or would save when Home Assistant stops."""
    # The hub saves with a delay. This event makes every store write at once.
    hass.bus.async_fire(EVENT_HOMEASSISTANT_FINAL_WRITE)
    await hass.async_block_till_done()
    return hass_storage[f"{DOMAIN}.{entry.entry_id}"]["data"]


def state_of(hass: HomeAssistant, entry: MockConfigEntry, platform: str, key: str):
    """The state of the entity for a declared key."""
    registered = entity_id(hass, entry, platform, key)
    assert registered is not None
    return hass.states.get(registered).state


# ── the frame ────────────────────────────────────────────────────


def test_scope_machine_is_a_machine_declaration() -> None:
    assert is_machine_declare({"t": "declare", "scope": "machine"})


def test_declaration_without_a_scope_is_full() -> None:
    assert not is_machine_declare({"t": "declare"})


@pytest.mark.parametrize("scope", NOT_MACHINE_SCOPES)
def test_every_other_scope_is_full(scope: Any) -> None:
    assert not is_machine_declare({"t": "declare", "scope": scope})


def test_declared_keys_skip_items_without_a_usable_key() -> None:
    entities = [
        CPU_SENSOR,
        {"kind": "sensor"},
        {"key": "", "kind": "sensor"},
        {"key": 7, "kind": "sensor"},
        {"key": None},
        "lock",
        None,
        LOCK_BUTTON,
    ]

    assert declared_keys(entities) == {"cpu_usage", "lock"}


def test_merged_machine_declaration_adds_and_replaces_but_keeps_the_rest() -> None:
    renamed_cpu = {**CPU_SENSOR, "name": "Processor"}

    merged = merge_machine_declaration(
        [CPU_SENSOR, LOCK_BUTTON], [renamed_cpu, UPTIME_SENSOR]
    )

    assert merged == [renamed_cpu, UPTIME_SENSOR, LOCK_BUTTON]


# ── next to a stored full declaration ────────────────────────────


@pytest.mark.parametrize("scope", NOT_MACHINE_SCOPES)
async def test_declare_with_another_scope_replaces_the_declaration(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    scope: Any,
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])

    await client.send(
        {"t": "declare", "scope": scope, "device": DEVICE, "entities": [CPU_SENSOR]}
    )
    await client.sync()
    await hass.async_block_till_done()

    assert entity_id(hass, entry, "switch", "keep_awake") is None
    assert hub.presence.active_keys is None
    assert state_of(hass, entry, "sensor", "cpu_usage") != STATE_UNAVAILABLE


async def test_machine_declare_keeps_the_registry_and_the_full_declaration(
    hass: HomeAssistant,
    hass_storage: dict[str, Any],
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
) -> None:
    full = [CPU_SENSOR, KEEP_AWAKE_SWITCH, LOCK_BUTTON]
    client = await connect()
    await declare(hass, client, full)

    await declare_machine(hass, client, [CPU_SENSOR])

    for platform, key in (
        ("sensor", "cpu_usage"),
        ("switch", "keep_awake"),
        ("button", "lock"),
    ):
        assert entity_id(hass, entry, platform, key) is not None
    assert hub.entities == full
    assert hub.device_info == DEVICE
    assert await stored_declaration(hass, hass_storage, entry) == {
        "device": DEVICE,
        "entities": full,
        "scope": "full",
    }
    device = dr.async_get(hass).async_get_device_by_identifier(
        (DOMAIN, NODE_ID), entry.entry_id
    )
    assert device is not None
    assert device.name == "Test PC"
    assert device.sw_version == "1.2.3"


async def test_machine_session_serves_only_what_both_declarations_list(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await client.send({"t": "state", "s": {"cpu_usage": 10, "keep_awake": False}})

    # The user switched Uptime off in the app, so the full declaration lacks it.
    await declare_machine(hass, client, [CPU_SENSOR, UPTIME_SENSOR, LOCK_BUTTON])

    assert hub.presence.active_keys == {"cpu_usage"}
    assert entity_id(hass, entry, "sensor", "uptime") is None
    assert entity_id(hass, entry, "button", "lock") is None
    assert state_of(hass, entry, "sensor", "cpu_usage") == "10"
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE


async def test_stored_descriptor_wins_over_the_machine_one(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    sensor = entity_id(hass, entry, "sensor", "cpu_usage")
    assert sensor is not None

    await declare_machine(
        hass, client, [{**CPU_SENSOR, "name": "Processor", "unit": "MHz"}]
    )
    await client.send({"t": "state", "s": {"cpu_usage": 10}})
    await client.sync()

    assert hub.entities == [CPU_SENSOR]
    state = hass.states.get(sensor)
    assert state.state == "10"
    assert state.attributes["unit_of_measurement"] == "%"
    assert state.attributes["friendly_name"] == "Test PC CPU usage"


async def test_state_frame_updates_only_the_active_keys(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await client.send({"t": "state", "s": {"cpu_usage": 10, "keep_awake": False}})
    await declare_machine(hass, client, [CPU_SENSOR])

    await client.send({"t": "state", "s": {"cpu_usage": 55, "keep_awake": True}})
    await client.sync()

    assert state_of(hass, entry, "sensor", "cpu_usage") == "55"
    # The full declaration brings the switch back with the value from before.
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_OFF


async def test_command_for_an_active_key_reaches_the_client(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH, LOCK_BUTTON])
    await declare_machine(hass, client, [CPU_SENSOR, LOCK_BUTTON])
    button = entity_id(hass, entry, "button", "lock")

    press = hass.async_create_task(
        hass.services.async_call(
            "button", "press", {"entity_id": button}, blocking=True
        )
    )
    command = await client.receive()
    await client.send({"t": "ack", "id": command["id"], "ok": True})
    await press

    assert command == {
        "t": "cmd",
        "id": command["id"],
        "key": "lock",
        "action": "press",
    }


async def test_command_outside_the_active_keys_is_refused(
    hass: HomeAssistant, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await declare_machine(hass, client, [CPU_SENSOR])

    with pytest.raises(HomeAssistantError) as refused:
        await hub.async_command("keep_awake", "set", True)

    assert str(refused.value) == NEEDS_APP_ERROR
    assert "needs the Deskmate app with a signed-in user" in NEEDS_APP_ERROR
    # sync() fails if the hub sent the command anyway.
    await client.sync()


async def test_full_declare_clears_the_filter(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await client.send({"t": "state", "s": {"keep_awake": True}})
    await declare_machine(hass, client, [CPU_SENSOR])
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE

    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])

    assert hub.presence.active_keys is None
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_ON


async def test_end_of_the_session_clears_the_filter(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Callable[[], Awaitable[None]],
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await first.send({"t": "state", "s": {"cpu_usage": 10, "keep_awake": True}})
    await declare_machine(hass, first, [CPU_SENSOR])

    await first.close()
    await wait_until_offline()

    assert hub.presence.active_keys is None
    assert state_of(hass, entry, "sensor", "cpu_usage") == STATE_UNAVAILABLE
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_UNAVAILABLE
    # A client that declares nothing gets every entity back, as before.
    second = await connect()
    await second.sync()
    assert state_of(hass, entry, "sensor", "cpu_usage") == "10"
    assert state_of(hass, entry, "switch", "keep_awake") == STATE_ON


async def test_link_sensor_stays_on_during_a_machine_session(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [LINK_SENSOR, CPU_SENSOR])

    await declare_machine(hass, client, [CPU_SENSOR])

    assert state_of(hass, entry, "binary_sensor", "link") == STATE_ON


async def test_owned_value_is_not_put_back_outside_the_active_keys(
    hass: HomeAssistant,
    hub: DeskmateHub,
    connect: Connect,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # Home Assistant may send an owned value again on every update.
    monkeypatch.setattr(entity_module, "OWNED_PUSH_EVERY_S", 0)
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, {**KEEP_AWAKE_SWITCH, "restore": True}])
    await client.send({"t": "state", "s": {"keep_awake": True}})
    # The computer reports another value: Home Assistant puts its own back.
    await client.send({"t": "state", "s": {"keep_awake": False}})
    put_back = await client.receive()
    assert (put_back["key"], put_back["action"], put_back["value"]) == (
        "keep_awake",
        "set",
        True,
    )
    await client.send({"t": "ack", "id": put_back["id"], "ok": True})
    await client.sync()
    commands: list[str] = []

    async def record_command(key: str, action: str, value: Any = None) -> None:
        commands.append(key)

    monkeypatch.setattr(hub, "async_command", record_command)

    await declare_machine(hass, client, [CPU_SENSOR])
    await client.send({"t": "state", "s": {"cpu_usage": 10}})
    await client.sync()
    await hass.async_block_till_done()

    assert commands == []


# ── without a stored full declaration ────────────────────────────


async def test_machine_declare_is_the_declaration_when_nothing_is_stored(
    hass: HomeAssistant,
    hass_storage: dict[str, Any],
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
) -> None:
    machine = [CPU_SENSOR, LOCK_BUTTON]
    client = await connect()

    await declare_machine(hass, client, machine)
    await client.send({"t": "state", "s": {"cpu_usage": 10}})
    await client.sync()

    assert hub.presence.active_keys == {"cpu_usage", "lock"}
    assert state_of(hass, entry, "sensor", "cpu_usage") == "10"
    assert state_of(hass, entry, "button", "lock") != STATE_UNAVAILABLE
    assert await stored_declaration(hass, hass_storage, entry) == {
        "device": SERVICE_DEVICE,
        "entities": machine,
        "scope": "machine",
    }


async def test_full_declare_replaces_a_stored_machine_declaration(
    hass: HomeAssistant,
    hass_storage: dict[str, Any],
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
) -> None:
    full = [CPU_SENSOR, KEEP_AWAKE_SWITCH]
    client = await connect()
    await declare_machine(hass, client, [CPU_SENSOR, LOCK_BUTTON])

    await declare(hass, client, full)

    assert entity_id(hass, entry, "button", "lock") is None
    assert entity_id(hass, entry, "switch", "keep_awake") is not None
    assert hub.presence.active_keys is None
    assert await stored_declaration(hass, hass_storage, entry) == {
        "device": DEVICE,
        "entities": full,
        "scope": "full",
    }


async def test_later_machine_declare_adds_entities_and_removes_none(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await declare_machine(hass, client, [CPU_SENSOR, LOCK_BUTTON])

    await declare_machine(hass, client, [CPU_SENSOR, UPTIME_SENSOR])

    assert hub.entities == [CPU_SENSOR, UPTIME_SENSOR, LOCK_BUTTON]
    assert hub.presence.active_keys == {"cpu_usage", "uptime"}
    assert state_of(hass, entry, "sensor", "uptime") != STATE_UNAVAILABLE
    assert state_of(hass, entry, "button", "lock") == STATE_UNAVAILABLE


# ── after a restart of Home Assistant ────────────────────────────


async def restart_entry(hass: HomeAssistant, entry: MockConfigEntry) -> DeskmateHub:
    """Reload the entry, as a restart does, and return its new hub."""
    assert await hass.config_entries.async_reload(entry.entry_id)
    await hass.async_block_till_done()
    return hass.data[DOMAIN]["hubs"][entry.entry_id]


async def test_restored_machine_declaration_is_still_a_machine_one(
    hass: HomeAssistant,
    hass_storage: dict[str, Any],
    entry: MockConfigEntry,
    connect: Connect,
) -> None:
    first = await connect()
    await declare_machine(hass, first, [CPU_SENSOR])
    await stored_declaration(hass, hass_storage, entry)
    restarted = await restart_entry(hass, entry)
    second = await connect()

    await declare_machine(hass, second, [CPU_SENSOR, LOCK_BUTTON])

    assert restarted.presence.active_keys == {"cpu_usage", "lock"}
    assert entity_id(hass, entry, "button", "lock") is not None


async def test_declaration_stored_without_a_scope_is_restored_as_full(
    hass: HomeAssistant,
    hass_storage: dict[str, Any],
    entry: MockConfigEntry,
    connect: Connect,
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    stored = await stored_declaration(hass, hass_storage, entry)
    # Integrations up to 0.8.5 wrote the declaration without a scope.
    del stored["scope"]
    restarted = await restart_entry(hass, entry)
    second = await connect()

    await declare_machine(hass, second, [CPU_SENSOR, LOCK_BUTTON])

    assert restarted.presence.active_keys == {"cpu_usage"}
    assert entity_id(hass, entry, "button", "lock") is None
    assert restarted.entities == [CPU_SENSOR, KEEP_AWAKE_SWITCH]
