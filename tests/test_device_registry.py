"""Devices in the registry: one per config entry, and merging a duplicate entry."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from typing import Any

from aiohttp.test_utils import TestClient
from homeassistant.core import HomeAssistant
from homeassistant.helpers import device_registry as dr
from homeassistant.helpers import entity_registry as er
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import (
    CONF_CASCADE_KEY,
    CONF_KEY,
    CONF_MIN_VERSION,
    CONF_NODE_ID,
    DOMAIN,
    PROTO_V2,
)
from custom_components.deskmate_link.crypto import gen_psk
from custom_components.deskmate_link.device_compat import (
    async_entry_device,
    async_hand_device_over,
)
from custom_components.deskmate_link.presence import connection_unique_id

from .link_client import NODE_ID, Connect
from .test_session import CPU_SENSOR, KEEP_AWAKE_SWITCH, declare, entity_id

IDENTIFIER = (DOMAIN, NODE_ID)


def earlier_entry(hass: HomeAssistant) -> MockConfigEntry:
    """Another entry bound to the test computer, as an earlier pairing left it."""
    config_entry = MockConfigEntry(
        domain=DOMAIN,
        title=NODE_ID,
        data={
            CONF_NODE_ID: NODE_ID,
            CONF_KEY: gen_psk(),
            CONF_CASCADE_KEY: "",
            CONF_MIN_VERSION: PROTO_V2,
        },
    )
    config_entry.add_to_hass(hass)
    return config_entry


# ── the calls that differ between Home Assistant versions ────────


class RegistryBeforePerEntryDevices:
    """What the device registry offered while a device could have many entries."""

    def __init__(self) -> None:
        self.looked_up: list[set[tuple[str, str]]] = []
        self.updated: list[tuple[str, dict[str, Any]]] = []

    def async_get_device(
        self, identifiers: set[tuple[str, str]] | None = None
    ) -> str | None:
        assert identifiers is not None
        self.looked_up.append(identifiers)
        return "the one device with that identifier"

    def async_update_device(
        self, device_id: str, *, add_config_entry_id: str | None = None
    ) -> None:
        self.updated.append((device_id, {"add_config_entry_id": add_config_entry_id}))


def test_lookup_falls_back_to_the_registry_wide_one() -> None:
    registry = RegistryBeforePerEntryDevices()

    device = async_entry_device(registry, "entry-id", IDENTIFIER)

    assert device == "the one device with that identifier"
    assert registry.looked_up == [{IDENTIFIER}]


def test_handing_over_falls_back_to_sharing_the_device() -> None:
    registry = RegistryBeforePerEntryDevices()

    async_hand_device_over(registry, "device-id", "entry-id")

    assert registry.updated == [("device-id", {"add_config_entry_id": "entry-id"})]


async def test_lookup_returns_the_device_of_the_entry(
    hass: HomeAssistant, entry: MockConfigEntry
) -> None:
    registry = dr.async_get(hass)
    other = earlier_entry(hass)
    ours = registry.async_get_or_create(
        config_entry_id=entry.entry_id, identifiers={IDENTIFIER}
    )
    theirs = registry.async_get_or_create(
        config_entry_id=other.entry_id, identifiers={IDENTIFIER}
    )

    assert ours.id != theirs.id
    assert async_entry_device(registry, entry.entry_id, IDENTIFIER) == ours
    assert async_entry_device(registry, other.entry_id, IDENTIFIER) == theirs


async def test_handing_over_moves_the_device_to_the_entry(
    hass: HomeAssistant, entry: MockConfigEntry
) -> None:
    registry = dr.async_get(hass)
    other = earlier_entry(hass)
    theirs = registry.async_get_or_create(
        config_entry_id=other.entry_id, identifiers={IDENTIFIER}
    )

    async_hand_device_over(registry, theirs.id, entry.entry_id)

    assert async_entry_device(registry, other.entry_id, IDENTIFIER) is None
    moved = async_entry_device(registry, entry.entry_id, IDENTIFIER)
    assert moved is not None
    assert moved.id == theirs.id


# ── the Deskmate page ────────────────────────────────────────────


async def test_computer_list_names_the_device_of_its_own_entry(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    http: TestClient,
    admin_headers: dict[str, str],
) -> None:
    client = await connect()
    await declare(hass, client, [CPU_SENSOR])
    registry = dr.async_get(hass)
    ours = registry.async_get_device_by_identifier(IDENTIFIER, entry.entry_id)
    assert ours is not None
    # A device of another entry with the same identifier must not be mistaken
    # for it.
    registry.async_get_or_create(
        config_entry_id=earlier_entry(hass).entry_id, identifiers={IDENTIFIER}
    )

    response = await http.get("/api/deskmate_link/files", headers=admin_headers)

    assert response.status == 200
    computers = await response.json()
    assert [computer["entry_id"] for computer in computers] == [entry.entry_id]
    assert computers[0]["device_id"] == ours.id


# ── a duplicate entry for the same computer ──────────────────────


async def test_new_pairing_takes_over_the_device_and_entities_of_the_old_entry(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    devices = dr.async_get(hass)
    entities = er.async_get(hass)
    old = earlier_entry(hass)
    assert await hass.config_entries.async_setup(old.entry_id)
    await hass.async_block_till_done()
    first = await connect(old.data[CONF_KEY])
    await declare(hass, first, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await first.close()
    device = devices.async_get_device_by_identifier(IDENTIFIER, old.entry_id)
    assert device is not None
    devices.async_update_device(device.id, name_by_user="Workshop")
    sensor = entity_id(hass, old, "sensor", "cpu_usage")
    switch = entity_id(hass, old, "switch", "keep_awake")
    connection = entities.async_get_entity_id(
        "sensor", DOMAIN, connection_unique_id(old.entry_id)
    )
    assert sensor is not None
    assert switch is not None
    assert connection is not None

    # The computer was paired again: it authenticates with the new entry's key.
    second = await connect()
    await second.sync()

    # The merge happens during the handshake. Look before a declaration could
    # put back what the merge lost.
    assert hass.config_entries.async_get_entry(old.entry_id) is None
    assert devices.async_get_device_by_identifier(IDENTIFIER, old.entry_id) is None
    kept = devices.async_get(device.id)
    assert kept is not None
    assert kept.name_by_user == "Workshop"
    owned = devices.async_get_device_by_identifier(IDENTIFIER, entry.entry_id)
    assert owned is not None
    assert owned.id == device.id
    for registered in (sensor, switch, connection):
        moved = entities.async_get(registered)
        assert moved.config_entry_id == entry.entry_id
        assert moved.unique_id.startswith(entry.entry_id)
        assert moved.device_id == device.id

    # The new entry's entities are the old ones: same entity ids, so the
    # history stays. That holds for the hub's own sensor too.
    await declare(hass, second, [CPU_SENSOR, KEEP_AWAKE_SWITCH])
    await second.send({"t": "state", "s": {"cpu_usage": 42}})
    await second.sync()
    assert entity_id(hass, entry, "sensor", "cpu_usage") == sensor
    assert entity_id(hass, entry, "switch", "keep_awake") == switch
    assert (
        entities.async_get_entity_id(
            "sensor", DOMAIN, connection_unique_id(entry.entry_id)
        )
        == connection
    )
    assert hass.states.get(sensor).state == "42"
    assert hass.states.get(connection).state == "connected"


async def test_entities_of_a_duplicate_entry_move_to_the_device_that_stays(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    wait_until_offline: Callable[[], Awaitable[None]],
) -> None:
    devices = dr.async_get(hass)
    entities = er.async_get(hass)
    first = await connect()
    await declare(hass, first, [CPU_SENSOR])
    await first.close()
    await wait_until_offline()
    ours = devices.async_get_device_by_identifier(IDENTIFIER, entry.entry_id)
    assert ours is not None
    own_sensor = entity_id(hass, entry, "sensor", "cpu_usage")
    # A second entry for the same computer, with a device and entities of its
    # own: one that this entry has too, and one that it lacks.
    old = earlier_entry(hass)
    theirs = devices.async_get_or_create(
        config_entry_id=old.entry_id, identifiers={IDENTIFIER}
    )
    duplicate = entities.async_get_or_create(
        "sensor",
        DOMAIN,
        f"{old.entry_id}-cpu_usage",
        config_entry=old,
        device_id=theirs.id,
    )
    only_theirs = entities.async_get_or_create(
        "sensor",
        DOMAIN,
        f"{old.entry_id}-gpu_load",
        config_entry=old,
        device_id=theirs.id,
    )

    second = await connect()
    await second.sync()

    assert hass.config_entries.async_get_entry(old.entry_id) is None
    assert devices.async_get(theirs.id) is None
    assert devices.async_get_device_by_identifier(IDENTIFIER, entry.entry_id) == ours
    assert entities.async_get(duplicate.entity_id) is None
    assert entity_id(hass, entry, "sensor", "cpu_usage") == own_sensor
    assert entity_id(hass, entry, "sensor", "gpu_load") == only_theirs.entity_id
    moved = entities.async_get(only_theirs.entity_id)
    assert moved.config_entry_id == entry.entry_id
    assert moved.device_id == ours.id
