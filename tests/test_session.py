"""Session frames: what `declare` and the end of a session do to entities."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from typing import Any

from homeassistant.const import STATE_UNAVAILABLE
from homeassistant.core import HomeAssistant
from homeassistant.helpers import device_registry as dr
from homeassistant.helpers import entity_registry as er
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import DOMAIN
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import NODE_ID, Connect, LinkClient

DEVICE = {"name": "Test PC", "model": "Deskmate for Windows", "sw_version": "1.2.3"}
CPU_SENSOR = {"key": "cpu_usage", "kind": "sensor", "name": "CPU usage", "unit": "%"}
KEEP_AWAKE_SWITCH = {"key": "keep_awake", "kind": "switch", "name": "Keep awake"}
LOCK_BUTTON = {"key": "lock", "kind": "button", "name": "Lock"}


async def declare(
    hass: HomeAssistant, client: LinkClient, entities: list[dict[str, Any]]
) -> None:
    """Send a `declare` and wait until Home Assistant has added the entities."""
    await client.send({"t": "declare", "device": DEVICE, "entities": entities})
    await client.sync()
    await hass.async_block_till_done()


def entity_id(
    hass: HomeAssistant, entry: MockConfigEntry, platform: str, key: str
) -> str | None:
    """The entity registered for a declared key, if there is one."""
    return er.async_get(hass).async_get_entity_id(
        platform, DOMAIN, f"{entry.entry_id}-{key}"
    )


async def test_declare_creates_the_entities(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()

    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH, LOCK_BUTTON])

    for platform, key in (
        ("sensor", "cpu_usage"),
        ("switch", "keep_awake"),
        ("button", "lock"),
    ):
        registered = entity_id(hass, entry, platform, key)
        assert registered is not None
        assert hass.states.get(registered) is not None
    device = dr.async_get(hass).async_get_device_by_identifier(
        (DOMAIN, NODE_ID), entry.entry_id
    )
    assert device is not None
    assert device.name == "Test PC"
    assert device.sw_version == "1.2.3"


async def test_second_declare_without_a_key_removes_the_entity(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    removed = entity_id(hass, entry, "switch", "keep_awake")
    assert removed is not None

    await declare(hass, client, [CPU_SENSOR])

    assert entity_id(hass, entry, "switch", "keep_awake") is None
    assert hass.states.get(removed) is None
    assert entity_id(hass, entry, "sensor", "cpu_usage") is not None


async def test_second_session_replaces_the_first(
    hass: HomeAssistant, entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    first = await connect()
    await declare(hass, first, [CPU_SENSOR])
    sensor = entity_id(hass, entry, "sensor", "cpu_usage")
    assert sensor is not None

    second = await connect()

    await first.wait_closed()
    await second.send({"t": "state", "s": {"cpu_usage": 42}})
    await second.sync()
    # The second session took over: the hub is still available and listens to it.
    assert hub.available
    assert hass.states.get(sensor).state == "42"


async def test_entities_become_unavailable_when_the_session_ends(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Callable[[], Awaitable[None]],
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    await client.send({"t": "state", "s": {"cpu_usage": 42}})
    await client.sync()
    sensor = entity_id(hass, entry, "sensor", "cpu_usage")
    assert sensor is not None
    assert hass.states.get(sensor).state == "42"

    await client.close()
    await wait_until_offline()

    assert not hub.available
    assert hass.states.get(sensor).state == STATE_UNAVAILABLE
