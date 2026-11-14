"""Fixtures shared by the Deskmate integration tests."""

from __future__ import annotations

import asyncio
from collections.abc import AsyncGenerator, Awaitable, Callable

import pytest
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.dispatcher import async_dispatcher_connect
from pytest_homeassistant_custom_component.common import (
    MockConfigEntry,
    mock_component,
)
from pytest_homeassistant_custom_component.typing import ClientSessionGenerator

from custom_components.deskmate_link.const import (
    CONF_CASCADE_KEY,
    CONF_KEY,
    CONF_MIN_VERSION,
    CONF_NODE_ID,
    DOMAIN,
    PROTO_V2,
    SIGNAL_AVAILABLE,
    UNBOUND_TITLE,
)
from custom_components.deskmate_link.crypto import gen_psk
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import NODE_ID, Connect, LinkClient

WAIT_TIMEOUT_S = 5


@pytest.fixture(autouse=True)
def auto_enable_custom_integrations(enable_custom_integrations: None) -> None:
    """Let Home Assistant load the integration from custom_components/."""


@pytest.fixture(autouse=True)
def stub_frontend(hass: HomeAssistant) -> None:
    """Mark `frontend` as set up without loading it.

    The integration depends on `frontend` for its sidebar page. The real
    component needs the built web interface (the home-assistant-frontend
    package), which no test here serves. Registering the page only writes to
    `hass.data`, so it works against the stub; `http` and `panel_custom` are
    set up for real.
    """
    mock_component(hass, "frontend")


@pytest.fixture
async def entry(hass: HomeAssistant) -> MockConfigEntry:
    """An entry as the config flow creates it: set up and waiting for pairing."""
    config_entry = MockConfigEntry(
        domain=DOMAIN,
        title=UNBOUND_TITLE,
        data={
            CONF_NODE_ID: "",
            CONF_KEY: gen_psk(),
            CONF_CASCADE_KEY: "",
            CONF_MIN_VERSION: PROTO_V2,
        },
    )
    config_entry.add_to_hass(hass)
    assert await hass.config_entries.async_setup(config_entry.entry_id)
    await hass.async_block_till_done()
    return config_entry


@pytest.fixture
def hub(hass: HomeAssistant, entry: MockConfigEntry) -> DeskmateHub:
    """The hub that the integration created for the entry."""
    return hass.data[DOMAIN]["hubs"][entry.entry_id]


@pytest.fixture
async def connect(
    hass_client_no_auth: ClientSessionGenerator, entry: MockConfigEntry
) -> AsyncGenerator[Connect]:
    """Open Link sessions as the computer `NODE_ID`, through the real view.

    Without an argument the session uses the entry's pairing key.
    """
    http = await hass_client_no_auth()
    clients: list[LinkClient] = []

    async def _connect(key: str | None = None) -> LinkClient:
        client = await LinkClient.connect(http, NODE_ID, key or entry.data[CONF_KEY])
        clients.append(client)
        return client

    yield _connect
    for client in clients:
        await client.close()


@pytest.fixture
def wait_until_offline(
    hass: HomeAssistant, hub: DeskmateHub
) -> Callable[[], Awaitable[None]]:
    """Wait until the hub has noticed that its session ended."""

    async def _wait() -> None:
        changed = asyncio.Event()

        @callback
        def _on_change() -> None:
            changed.set()

        unsubscribe = async_dispatcher_connect(
            hass, f"{SIGNAL_AVAILABLE}_{hub.entry.entry_id}", _on_change
        )
        try:
            async with asyncio.timeout(WAIT_TIMEOUT_S):
                while hub.available:
                    await changed.wait()
                    changed.clear()
        finally:
            unsubscribe()

    return _wait
