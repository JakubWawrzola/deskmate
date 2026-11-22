"""Notification tags and priorities, and `deskmate_link.clear_notification`."""

from __future__ import annotations

from collections.abc import AsyncGenerator, Awaitable, Callable
from typing import Any

import pytest
import voluptuous as vol
from aiohttp.test_utils import TestClient
from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link import hub as hub_module
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
from custom_components.deskmate_link.notify_tag import (
    NO_ANSWER_TO_CLEAR_ERROR,
    NO_ANSWER_TO_NOTIFY_REASON,
    NOT_CONNECTED_TO_CLEAR_ERROR,
    NOT_CONNECTED_TO_NOTIFY_REASON,
    REFUSED_TO_CLEAR_ERROR,
    REFUSED_TO_NOTIFY_REASON,
    TOO_OLD_TO_CLEAR_ERROR,
    notify_clear_frame,
    notify_failures_error,
    notify_frame,
    validate_tag,
)
from custom_components.deskmate_link.service_role import SERVICE_ONLY_ERROR

from .link_client import NODE_ID, Connect, LinkClient

INFO = {"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
NOTIFICATION = {"title": "Doorbell", "message": "Someone is at the door."}
TAGGED = {**NOTIFICATION, "tag": "doorbell", "priority": "urgent"}
# The two actions that go to every computer, with the frame each one sends.
TAGGED_ACTIONS = [
    ("notify", TAGGED, "notify"),
    ("clear_notification", {"tag": "doorbell"}, "notify_clear"),
]
# Sessions that must get neither the two fields nor `notify_clear`: no
# `info` at all, `info` without `features`, and features without this one.
OLDER_CLIENTS = [None, [], ["media_player"]]
VALID_TAGS = ["a", "Z", "0", "door_bell-2", "x" * 64]
INVALID_TAGS = ["", "x" * 65, "door bell", "door.bell", "doorbell\n", "dörbell", "#1"]
# A second paired computer, set up after the one in the `entry` fixture.
SECOND_NODE_ID = "desk_pc"
SHORT_ACK_TIMEOUT_S = 0.05


@pytest.fixture
async def second_computer(
    hass: HomeAssistant, entry: MockConfigEntry, http: TestClient
) -> AsyncGenerator[LinkClient]:
    """A session of `SECOND_NODE_ID` that lists `notify_tag`, on its own entry."""
    second_entry = MockConfigEntry(
        domain=DOMAIN,
        title=UNBOUND_TITLE,
        data={
            CONF_NODE_ID: "",
            CONF_KEY: gen_psk(),
            CONF_CASCADE_KEY: "",
            CONF_MIN_VERSION: PROTO_V2,
        },
    )
    second_entry.add_to_hass(hass)
    assert await hass.config_entries.async_setup(second_entry.entry_id)
    await hass.async_block_till_done()
    client = await LinkClient.connect(http, SECOND_NODE_ID, second_entry.data[CONF_KEY])
    await client.send({**INFO, "features": ["notify_tag"]})
    assert (await client.receive())["t"] == "info"
    yield client
    await client.close()


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


@pytest.mark.parametrize("placement", ["hero", "inline"])
def test_image_placement_goes_to_every_client_with_the_image(placement: str) -> None:
    for features in (frozenset(), frozenset({"notify_tag"})):
        frame = notify_frame(
            features, "t", "m", "https://ha.example/door.jpg", image_placement=placement
        )

        assert frame == {
            "t": "notify",
            "title": "t",
            "message": "m",
            "image": "https://ha.example/door.jpg",
            "image_placement": placement,
        }


@pytest.mark.parametrize("placement", [None, "logo"])
def test_default_image_placement_leaves_the_frame_as_it_was(
    placement: str | None,
) -> None:
    frame = notify_frame(
        frozenset(), "t", "m", "https://ha.example/door.jpg", image_placement=placement
    )

    assert frame == {
        "t": "notify",
        "title": "t",
        "message": "m",
        "image": "https://ha.example/door.jpg",
    }


def test_image_placement_without_an_image_is_left_out() -> None:
    frame = notify_frame(frozenset(), "t", "m", image_placement="hero")

    assert frame == {"t": "notify", "title": "t", "message": "m"}


@pytest.mark.parametrize("placement", ["", "Hero", "banner", "top", 1])
def test_invalid_image_placement_is_refused(placement: Any) -> None:
    with pytest.raises(HomeAssistantError):
        notify_frame(
            frozenset(),
            "t",
            "m",
            "https://ha.example/door.jpg",
            image_placement=placement,
        )


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

    with pytest.raises(HomeAssistantError) as refused:
        await call
    # The client's own text is not checked, so it does not reach the user.
    assert str(refused.value) == REFUSED_TO_CLEAR_ERROR


async def test_clear_notification_fails_when_the_client_does_not_answer(
    hass: HomeAssistant, connect: Connect, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(hub_module, "CMD_ACK_TIMEOUT", SHORT_ACK_TIMEOUT_S)
    client = await session(connect, ["notify_tag"])
    call = hass.async_create_task(
        hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )
    )
    assert (await client.receive())["t"] == "notify_clear"

    with pytest.raises(HomeAssistantError) as unanswered:
        await call
    assert str(unanswered.value) == NO_ANSWER_TO_CLEAR_ERROR


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
    hass: HomeAssistant,
    connect: Connect,
    wait_until_session_ended: Callable[[], Awaitable[None]],
) -> None:
    await first_computer_offline(connect, wait_until_session_ended)

    with pytest.raises(HomeAssistantError) as refused:
        await hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )

    assert str(refused.value) == NOT_CONNECTED_TO_CLEAR_ERROR


@pytest.mark.parametrize(
    ("service", "data"), [(service, data) for service, data, _ in TAGGED_ACTIONS]
)
async def test_an_action_without_a_paired_computer_fails(
    hass: HomeAssistant, entry: MockConfigEntry, service: str, data: dict[str, Any]
) -> None:
    with pytest.raises(HomeAssistantError) as refused:
        await hass.services.async_call(DOMAIN, service, data, blocking=True)

    assert str(refused.value) == "Deskmate: no paired computer"


@pytest.mark.parametrize(("service", "data", "frame_type"), TAGGED_ACTIONS)
async def test_an_entry_waiting_for_pairing_is_not_a_target(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    second_computer: LinkClient,
    service: str,
    data: dict[str, Any],
    frame_type: str,
) -> None:
    # `entry` was set up first and never paired: a loop that counted it
    # would fail, or stop, before the computer that is paired.
    frame = await answered(hass, second_computer, service, data)

    assert frame["t"] == frame_type
    assert frame["tag"] == "doorbell"


async def first_computer_offline(
    connect: Connect, wait_until_session_ended: Callable[[], Awaitable[None]]
) -> None:
    """Paired as `NODE_ID`, then gone, like a laptop that went to sleep."""
    client = await session(connect, ["notify_tag"])
    await client.close()
    await wait_until_session_ended()


async def first_computer_too_old(
    connect: Connect, wait_until_session_ended: Callable[[], Awaitable[None]]
) -> None:
    await session(connect, [])


async def first_computer_service_only(
    connect: Connect, wait_until_session_ended: Callable[[], Awaitable[None]]
) -> None:
    await session(connect, ["notify_tag"], role="service")


@pytest.mark.parametrize(
    ("first_computer", "reason"),
    [
        (first_computer_offline, NOT_CONNECTED_TO_CLEAR_ERROR),
        (first_computer_too_old, TOO_OLD_TO_CLEAR_ERROR),
        (first_computer_service_only, SERVICE_ONLY_ERROR),
    ],
    ids=["offline", "too_old", "service_only"],
)
async def test_clear_notification_reaches_every_computer_when_one_cannot(
    hass: HomeAssistant,
    connect: Connect,
    wait_until_session_ended: Callable[[], Awaitable[None]],
    second_computer: LinkClient,
    first_computer: Callable[..., Awaitable[None]],
    reason: str,
) -> None:
    # The computer that fails comes first, so a loop that stops at it would
    # never reach the second one.
    await first_computer(connect, wait_until_session_ended)
    call = hass.async_create_task(
        hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )
    )

    frame = await second_computer.receive()
    await second_computer.send({"t": "ack", "id": frame["id"], "ok": True})

    assert frame == {"t": "notify_clear", "id": frame["id"], "tag": "doorbell"}
    with pytest.raises(HomeAssistantError) as refused:
        await call
    message = str(refused.value)
    assert message.startswith(
        "Deskmate: the notification was not cleared on 1 of 2 computers."
    )
    assert f"{NODE_ID}: {reason.removeprefix('Deskmate: ')}" in message
    assert SECOND_NODE_ID not in message


@pytest.mark.parametrize(
    ("first_computer", "reason"),
    [
        (first_computer_offline, NOT_CONNECTED_TO_NOTIFY_REASON),
        (first_computer_service_only, SERVICE_ONLY_ERROR.removeprefix("Deskmate: ")),
    ],
    ids=["offline", "service_only"],
)
async def test_notify_reaches_every_computer_when_one_cannot(
    hass: HomeAssistant,
    connect: Connect,
    wait_until_session_ended: Callable[[], Awaitable[None]],
    second_computer: LinkClient,
    first_computer: Callable[..., Awaitable[None]],
    reason: str,
) -> None:
    # The computer that fails comes first, so a loop that stops at it would
    # never reach the second one.
    await first_computer(connect, wait_until_session_ended)
    call = hass.async_create_task(
        hass.services.async_call(DOMAIN, "notify", NOTIFICATION, blocking=True)
    )

    frame = await second_computer.receive()
    await second_computer.send({"t": "ack", "id": frame["id"], "ok": True})

    assert frame == {"t": "notify", "id": frame["id"], **NOTIFICATION}
    with pytest.raises(HomeAssistantError) as refused:
        await call
    message = str(refused.value)
    assert message.startswith(
        "Deskmate: the notification was not shown on 1 of 2 computers."
    )
    assert f"{NODE_ID}: {reason}" in message
    assert SECOND_NODE_ID not in message


@pytest.mark.parametrize(
    ("error", "reason"),
    [
        (ConnectionError("deskmate not connected"), NOT_CONNECTED_TO_NOTIFY_REASON),
        (TimeoutError(), NO_ANSWER_TO_NOTIFY_REASON),
        (RuntimeError("<b>text from the client</b>"), REFUSED_TO_NOTIFY_REASON),
        (
            HomeAssistantError(SERVICE_ONLY_ERROR),
            SERVICE_ONLY_ERROR.removeprefix("Deskmate: "),
        ),
    ],
    ids=["offline", "no_answer", "refused", "service_only"],
)
def test_notify_failures_name_each_computer_without_the_client_text(
    error: Exception, reason: str
) -> None:
    combined = notify_failures_error(3, [("my_pc", error), ("desk_pc", error)])

    assert isinstance(combined, HomeAssistantError)
    assert str(combined) == (
        "Deskmate: the notification was not shown on 2 of 3 computers. "
        f"my_pc: {reason}; desk_pc: {reason}"
    )


def test_notify_failure_of_the_only_computer_keeps_its_own_error() -> None:
    error = ConnectionError("deskmate not connected")

    assert notify_failures_error(1, [("my_pc", error)]) is error
    assert notify_failures_error(2, []) is None


async def test_clear_notification_reaches_every_connected_computer(
    hass: HomeAssistant, connect: Connect, second_computer: LinkClient
) -> None:
    first = await session(connect, ["notify_tag"])
    call = hass.async_create_task(
        hass.services.async_call(
            DOMAIN, "clear_notification", {"tag": "doorbell"}, blocking=True
        )
    )

    for client in (first, second_computer):
        frame = await client.receive()
        assert frame == {"t": "notify_clear", "id": frame["id"], "tag": "doorbell"}
        await client.send({"t": "ack", "id": frame["id"], "ok": True})
    await call


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
