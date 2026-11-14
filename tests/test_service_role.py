"""What Home Assistant refuses while only the Windows service is connected."""

from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

import pytest
from aiohttp.test_utils import TestClient
from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import DOMAIN
from custom_components.deskmate_link.service_role import SERVICE_ONLY_ERROR

from .link_client import Connect, LinkClient

INFO = {"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
NOTIFICATION = {"title": "Laundry done", "message": "Take it out of the washer."}
# Every action of the Files API, with the method that the page uses for it.
FILES_ACTIONS = [
    ("GET", "roots"),
    ("GET", "list"),
    ("GET", "download"),
    ("POST", "upload_start"),
    ("POST", "upload_chunk"),
    ("POST", "upload_finish"),
    ("POST", "upload_abort"),
]
# Clients that are not the service: the two other roles, and no `info` at all.
OTHER_CLIENTS = ["app", "daemon", None]


async def session(connect: Connect, role: str | None) -> LinkClient:
    """Open a session and, for a role, say so with `info`."""
    client = await connect()
    if role is not None:
        await client.send({**INFO, "role": role})
        assert (await client.receive())["t"] == "info"
    await client.sync()
    return client


def files_url(entry: MockConfigEntry, action: str) -> str:
    return f"/api/deskmate_link/files/{entry.entry_id}/{action}"


def test_the_error_says_what_is_needed() -> None:
    assert "notifications and files need the Deskmate app" in SERVICE_ONLY_ERROR
    assert "signed-in user" in SERVICE_ONLY_ERROR


# ── notifications ────────────────────────────────────────────────


async def test_notify_is_refused_while_the_service_is_connected(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, "service")

    with pytest.raises(HomeAssistantError) as refused:
        await hass.services.async_call(DOMAIN, "notify", NOTIFICATION, blocking=True)

    assert str(refused.value) == SERVICE_ONLY_ERROR
    # sync() fails if the hub sent the notification anyway.
    await client.sync()


@pytest.mark.parametrize("role", OTHER_CLIENTS)
async def test_notify_reaches_every_other_client(
    hass: HomeAssistant, connect: Connect, role: str | None
) -> None:
    client = await session(connect, role)

    call = hass.async_create_task(
        hass.services.async_call(DOMAIN, "notify", NOTIFICATION, blocking=True)
    )
    frame = await client.receive()
    await client.send({"t": "ack", "id": frame["id"], "ok": True})
    await call

    assert frame == {"t": "notify", "id": frame["id"], **NOTIFICATION}


async def test_notify_after_the_service_left_fails_as_not_connected(
    hass: HomeAssistant,
    connect: Connect,
    wait_until_session_ended: Callable[[], Awaitable[None]],
) -> None:
    client = await session(connect, "service")
    await client.send({"t": "bye", "reason": "handover"})
    await client.close()
    await wait_until_session_ended()

    with pytest.raises(ConnectionError, match="deskmate not connected"):
        await hass.services.async_call(DOMAIN, "notify", NOTIFICATION, blocking=True)


# ── file services ────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("service", "data"),
    [
        ("send_file", {"path": "/media/picture.jpg"}),
        ("fetch_file", {"path": "Documents/report.pdf", "destination": "/media"}),
    ],
)
async def test_file_services_are_refused_while_the_service_is_connected(
    hass: HomeAssistant, connect: Connect, service: str, data: dict[str, Any]
) -> None:
    client = await session(connect, "service")

    with pytest.raises(HomeAssistantError) as refused:
        await hass.services.async_call(DOMAIN, service, data, blocking=True)

    assert str(refused.value) == SERVICE_ONLY_ERROR
    await client.sync()


@pytest.mark.parametrize("role", OTHER_CLIENTS)
async def test_fetch_file_asks_every_other_client(
    hass: HomeAssistant, connect: Connect, role: str | None
) -> None:
    client = await session(connect, role)

    call = hass.async_create_task(
        hass.services.async_call(
            DOMAIN,
            "fetch_file",
            {"path": "Documents/report.pdf", "destination": "/media"},
            blocking=True,
        )
    )
    frame = await client.receive()
    await client.send(
        {"t": "fs_res", "id": frame["id"], "ok": False, "error": "not shared"}
    )

    assert frame == {
        "t": "fs",
        "id": frame["id"],
        "op": "stat",
        "path": "Documents/report.pdf",
    }
    with pytest.raises(HomeAssistantError, match="Deskmate Files: not shared"):
        await call


@pytest.mark.parametrize("role", OTHER_CLIENTS)
async def test_send_file_gets_past_the_role_for_every_other_client(
    hass: HomeAssistant, connect: Connect, tmp_path: Path, role: str | None
) -> None:
    await session(connect, role)
    outside = tmp_path / "picture.jpg"

    # The next check after the role is the one on the path.
    with pytest.raises(HomeAssistantError, match="allowlist_external_dirs"):
        await hass.services.async_call(
            DOMAIN, "send_file", {"path": str(outside)}, blocking=True
        )


# ── the Files API of the Deskmate page ───────────────────────────


@pytest.mark.parametrize(("method", "action"), FILES_ACTIONS)
async def test_files_api_is_refused_while_the_service_is_connected(
    entry: MockConfigEntry,
    connect: Connect,
    http: TestClient,
    admin_headers: dict[str, str],
    method: str,
    action: str,
) -> None:
    client = await session(connect, "service")

    response = await http.request(
        method, files_url(entry, action), headers=admin_headers
    )

    assert response.status == 502
    assert await response.json() == {"message": SERVICE_ONLY_ERROR}
    await client.sync()


@pytest.mark.parametrize("role", OTHER_CLIENTS)
async def test_files_api_asks_every_other_client(
    entry: MockConfigEntry,
    connect: Connect,
    http: TestClient,
    admin_headers: dict[str, str],
    role: str | None,
) -> None:
    client = await session(connect, role)

    request = asyncio.create_task(
        http.get(files_url(entry, "roots"), headers=admin_headers)
    )
    frame = await client.receive()
    await client.send(
        {
            "t": "fs_res",
            "id": frame["id"],
            "ok": True,
            "roots": [{"name": "Documents"}],
            "inbox": {"mode": "confirm"},
            "max_bytes": 1024,
        }
    )
    response = await request

    assert frame == {"t": "fs", "id": frame["id"], "op": "roots"}
    assert response.status == 200
    assert await response.json() == {
        "roots": [{"name": "Documents"}],
        "inbox": {"mode": "confirm"},
        "max_bytes": 1024,
    }


async def test_files_api_still_wants_an_administrator_first(
    entry: MockConfigEntry, connect: Connect, http: TestClient
) -> None:
    await session(connect, "service")

    response = await http.get(files_url(entry, "roots"))

    assert response.status == 401
