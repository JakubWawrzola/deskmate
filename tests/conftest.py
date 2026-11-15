"""Fixtures shared by the Deskmate integration tests."""

from __future__ import annotations

import asyncio
from collections.abc import AsyncGenerator, Awaitable, Callable, Generator

import pytest
from aiohttp.test_utils import TestClient
from homeassistant.core import HomeAssistant
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
    UNBOUND_TITLE,
)
from custom_components.deskmate_link.crypto import gen_psk
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import NODE_ID, Connect, LinkClient

WAIT_TIMEOUT_S = 5
# The logger that Home Assistant reports deprecated and wrong calls on.
USAGE_REPORT_LOGGER = "homeassistant.helpers.frame"


@pytest.fixture(autouse=True)
def no_reported_usage(caplog: pytest.LogCaptureFixture) -> Generator[None]:
    """Fail a test in which Home Assistant reports a call it is going to drop.

    This is how a deprecated registry call shows up before it stops working.
    """
    yield
    reports = [
        record.getMessage()
        for phase in ("setup", "call")
        for record in caplog.get_records(phase)
        if record.name == USAGE_REPORT_LOGGER
    ]
    assert not reports


@pytest.fixture
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
async def entry(
    hass: HomeAssistant, enable_custom_integrations: None, stub_frontend: None
) -> MockConfigEntry:
    """An entry as the config flow creates it: set up and waiting for pairing.

    `enable_custom_integrations` lets Home Assistant load the integration from
    custom_components/.
    """
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
async def http(
    hass_client_no_auth: ClientSessionGenerator, entry: MockConfigEntry
) -> TestClient:
    """An HTTP client for Home Assistant that logs in as nobody.

    A request that needs a user adds `admin_headers`.
    """
    return await hass_client_no_auth()


@pytest.fixture
def admin_headers(hass_access_token: str) -> dict[str, str]:
    """Headers that make a request come from an administrator."""
    return {"Authorization": f"Bearer {hass_access_token}"}


@pytest.fixture
async def connect(http: TestClient, entry: MockConfigEntry) -> AsyncGenerator[Connect]:
    """Open Link sessions as the computer `NODE_ID`, through the real view.

    Without an argument the session uses the entry's pairing key.
    """
    clients: list[LinkClient] = []

    async def _connect(key: str | None = None) -> LinkClient:
        client = await LinkClient.connect(http, NODE_ID, key or entry.data[CONF_KEY])
        clients.append(client)
        return client

    yield _connect
    for client in clients:
        await client.close()


@pytest.fixture
def wait_until_offline(hub: DeskmateHub) -> Callable[[], Awaitable[None]]:
    """Wait until the hub has noticed that its session ended for good.

    A session that never declared was never available, so the flag alone
    does not tell that the socket closed.
    """

    async def _wait() -> None:
        async with asyncio.timeout(WAIT_TIMEOUT_S):
            while hub.connected or hub.available:  # noqa: ASYNC110 - no event to wait for
                await asyncio.sleep(0.01)

    return _wait


@pytest.fixture
def wait_until_session_ended(hub: DeskmateHub) -> Callable[[], Awaitable[None]]:
    """Wait until the hub has noticed that its socket closed.

    Unlike `wait_until_offline`, this also returns when the computer stays
    available after the session, as it does during a handover.
    """

    async def _wait() -> None:
        async with asyncio.timeout(WAIT_TIMEOUT_S):
            # The hub signals nothing when a socket closes during a handover.
            while hub.connected:  # noqa: ASYNC110 - no event to wait for
                await asyncio.sleep(0.01)

    return _wait
