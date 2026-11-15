"""Notification tags and priorities, and `deskmate_link.clear_notification`."""

from __future__ import annotations

from typing import Any

import pytest
import voluptuous as vol
from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import DOMAIN
from custom_components.deskmate_link.notify_tag import (
    TOO_OLD_TO_CLEAR_ERROR,
    notify_clear_frame,
    notify_frame,
    validate_tag,
)
from custom_components.deskmate_link.service_role import SERVICE_ONLY_ERROR

from .link_client import Connect, LinkClient

INFO = {"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
NOTIFICATION = {"title": "Doorbell", "message": "Someone is at the door."}
TAGGED = {**NOTIFICATION, "tag": "doorbell", "priority": "urgent"}
# Sessions that must get neither the two fields nor `notify_clear`: no
# `info` at all, `info` without `features`, and features without this one.
OLDER_CLIENTS = [None, [], ["media_player"]]
VALID_TAGS = ["a", "Z", "0", "door_bell-2", "x" * 64]
INVALID_TAGS = ["", "x" * 65, "door bell", "door.bell", "doorbell\n", "dörbell", "#1"]


async def session(
    connect: Connect, features: list[str] | None, role: str = "app"
) -> LinkClient:
    """Open a session; with `features` it sends `info` that lists them."""
    client = await connect()
    if features is not None:
        await client.send({**INFO, "role": role, "features": features})
        assert (await client.receive())["t"] == "info"
    return client


async def answered(
    hass: HomeAssistant, client: LinkClient, service: str, data: dict[str, Any]
) -> dict[str, Any]:
    """Call the action, acknowledge the frame it sends and return that frame."""
    call = hass.async_create_task(
        hass.services.async_call(DOMAIN, service, data, blocking=True)
    )
    frame = await client.receive()
    assert not call.done()
    await client.send({"t": "ack", "id": frame["id"], "ok": True})
    await call
    return frame


# ── validation ───────────────────────────────────────────────────


@pytest.mark.parametrize("tag", VALID_TAGS)
def test_valid_tag_is_accepted(tag: str) -> None:
    assert validate_tag(tag) == tag
    assert notify_clear_frame(tag) == {"t": "notify_clear", "tag": tag}


@pytest.mark.parametrize("tag", [*INVALID_TAGS, None, 7, ["doorbell"]])
def test_invalid_tag_is_refused(tag: Any) -> None:
    with pytest.raises(vol.Invalid):
        validate_tag(tag)
    with pytest.raises(HomeAssistantError):
        notify_clear_frame(tag)
    if tag is not None:
        # For a notification, no tag is a valid choice.
        with pytest.raises(HomeAssistantError):
            notify_frame(frozenset({"notify_tag"}), "t", "m", tag=tag)


@pytest.mark.parametrize("priority", ["", "Normal", "low", "critical", 1])
def test_invalid_priority_is_refused_for_every_client(priority: Any) -> None:
    for features in (frozenset(), frozenset({"notify_tag"})):
        with pytest.raises(HomeAssistantError):
            notify_frame(features, "t", "m", priority=priority)


@pytest.mark.parametrize("priority", ["normal", "high", "urgent"])
def test_priority_and_tag_go_to_a_client_that_lists_the_feature(
    priority: str,
) -> None:
    frame = notify_frame(
        frozenset({"notify_tag"}), "t", "m", tag="doorbell", priority=priority
    )

    assert frame == {
        "t": "notify",
        "title": "t",
        "message": "m",
        "tag": "doorbell",
        "priority": priority,
    }


def test_older_client_gets_the_notification_without_tag_and_priority() -> None:
    frame = notify_frame(
        frozenset({"media_player"}), "t", "m", tag="doorbell", priority="high"
    )

    assert frame == {"t": "notify", "title": "t", "message": "m"}


# ── deskmate_link.notify ─────────────────────────────────────────


async def test_notify_sends_tag_and_priority_to_a_client_that_lists_them(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, ["notify_tag"])

    frame = await answered(hass, client, "notify", TAGGED)

    assert frame == {"t": "notify", "id": frame["id"], **TAGGED}


async def test_notify_leaves_out_what_was_not_set(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, ["notify_tag", "media_player"])

    frame = await answered(hass, client, "notify", {**NOTIFICATION, "tag": "door"})

    assert frame == {"t": "notify", "id": frame["id"], **NOTIFICATION, "tag": "door"}


@pytest.mark.parametrize("features", OLDER_CLIENTS)
async def test_notify_drops_tag_and_priority_for_an_older_client(
    hass: HomeAssistant, connect: Connect, features: list[str] | None
) -> None:
    client = await session(connect, features)

    frame = await answered(hass, client, "notify", TAGGED)

    assert frame == {"t": "notify", "id": frame["id"], **NOTIFICATION}


@pytest.mark.parametrize(
    "changes",
    [{"tag": tag} for tag in INVALID_TAGS]
    + [{"priority": "low"}, {"priority": "URGENT"}, {"priority": ""}],
)
async def test_notify_with_an_invalid_field_sends_nothing(
    hass: HomeAssistant, connect: Connect, changes: dict[str, Any]
) -> None:
    client = await session(connect, ["notify_tag"])

    with pytest.raises(vol.Invalid):
        await hass.services.async_call(
            DOMAIN, "notify", {**NOTIFICATION, **changes}, blocking=True
        )

    # sync() fails if the hub sent the notification anyway.
    await client.sync()


# ── deskmate_link.clear_notification ─────────────────────────────


async def test_clear_notification_reaches_a_client_that_lists_the_feature(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, ["notify_tag"])

    frame = await answered(hass, client, "clear_notification", {"tag": "doorbell"})

    assert frame == {"t": "notify_clear", "id": frame["id"], "tag": "doorbell"}


async def test_clear_notification_fails_when_the_client_refuses(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, ["notify_tag"])
    call = hass.async_create_task(
        hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )
    )
    frame = await client.receive()

    await client.send({"t": "ack", "id": frame["id"], "ok": False, "error": "busy"})

    with pytest.raises(RuntimeError, match="busy"):
        await call


@pytest.mark.parametrize("features", OLDER_CLIENTS)
async def test_clear_notification_tells_that_an_older_client_cannot(
    hass: HomeAssistant, connect: Connect, features: list[str] | None
) -> None:
    client = await session(connect, features)

    with pytest.raises(HomeAssistantError) as refused:
        await hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )

    assert str(refused.value) == TOO_OLD_TO_CLEAR_ERROR
    assert "too old to clear notifications" in TOO_OLD_TO_CLEAR_ERROR
    # sync() fails if the hub sent `notify_clear` anyway.
    await client.sync()


async def test_clear_notification_is_refused_while_the_service_is_connected(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, ["notify_tag"], role="service")

    with pytest.raises(HomeAssistantError) as refused:
        await hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )

    assert str(refused.value) == SERVICE_ONLY_ERROR
    await client.sync()


async def test_clear_notification_without_a_session_fails_as_not_connected(
    hass: HomeAssistant, entry: MockConfigEntry
) -> None:
    with pytest.raises(ConnectionError, match="deskmate not connected"):
        await hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )


async def test_clear_notification_for_an_unknown_computer_fails(
    hass: HomeAssistant, connect: Connect
) -> None:
    client = await session(connect, ["notify_tag"])

    with pytest.raises(HomeAssistantError, match="no paired computer"):
        await hass.services.async_call(
            DOMAIN,
            "clear_notification",
            {"tag": "doorbell", "node_id": "other_pc"},
            blocking=True,
        )

    await client.sync()


@pytest.mark.parametrize("data", [{}, *({"tag": tag} for tag in INVALID_TAGS)])
async def test_clear_notification_needs_a_valid_tag(
    hass: HomeAssistant, connect: Connect, data: dict[str, Any]
) -> None:
    client = await session(connect, ["notify_tag"])

    with pytest.raises(vol.Invalid):
        await hass.services.async_call(
            DOMAIN, "clear_notification", data, blocking=True
        )

    await client.sync()
