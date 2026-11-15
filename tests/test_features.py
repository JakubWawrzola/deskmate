"""The `features` of an `info` frame: what a client says it implements."""

from __future__ import annotations

import logging
from collections.abc import Awaitable, Callable
from typing import Any

import pytest
from homeassistant.core import HomeAssistant
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.diagnostics import (
    async_get_config_entry_diagnostics,
)
from custom_components.deskmate_link.hub import DeskmateHub
from custom_components.deskmate_link.session_info import client_features

from .link_client import NODE_ID, Connect

INFO = {"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
BOTH = ["media_player", "notify_tag"]
HUB_LOGGER = "custom_components.deskmate_link.hub"


# ── validation ───────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("features", "known"),
    [
        ([], set()),
        (["notify_tag"], {"notify_tag"}),
        (BOTH, set(BOTH)),
        (["notify_tag", "notify_tag"], {"notify_tag"}),
        (["later_feature", "media_player"], {"media_player"}),
        (["a" * 32], set()),
        ([f"feature_{n}" for n in range(15)] + ["notify_tag"], {"notify_tag"}),
    ],
)
def test_known_features_are_kept_and_unknown_ones_left_out(
    features: list[str], known: set[str]
) -> None:
    assert client_features({**INFO, "features": features}) == known


def test_frame_without_features_lists_none() -> None:
    assert client_features(INFO) == frozenset()


@pytest.mark.parametrize(
    "features",
    [
        None,
        "notify_tag",
        {"notify_tag": True},
        7,
        [f"feature_{n}" for n in range(17)],
        ["notify_tag", 1],
        ["notify_tag", None],
        [["notify_tag"]],
        [""],
        ["Notify_tag"],
        ["notify-tag"],
        ["1notify"],
        ["notify_tag\n"],
        [" notify_tag"],
        ["a" * 33],
        ["notífy_tag"],
    ],
)
def test_invalid_features_are_refused(features: Any) -> None:
    assert client_features({**INFO, "features": features}) is None


# ── through the hub ──────────────────────────────────────────────


async def test_features_are_kept_for_the_session(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()

    await client.send({**INFO, "features": [*BOTH, "later_feature"]})

    assert (await client.receive())["t"] == "info"
    assert hub.session_features == set(BOTH)


async def test_info_without_features_lists_none(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()

    await client.send(INFO)

    assert (await client.receive())["t"] == "info"
    assert hub.session_role == "app"
    assert hub.session_features == frozenset()


async def test_invalid_features_leave_the_rest_of_the_info_in_place(
    hub: DeskmateHub, connect: Connect, caplog: pytest.LogCaptureFixture
) -> None:
    client = await connect()

    await client.send({**INFO, "features": ["notify_tag", "RECEIVED"]})

    assert (await client.receive())["t"] == "info"
    assert (hub.session_role, hub.session_app) == ("app", "0.9.0")
    assert hub.session_features == frozenset()
    warnings = [
        record.getMessage()
        for record in caplog.records
        if record.name == HUB_LOGGER and record.levelno == logging.WARNING
    ]
    assert warnings == [
        f"deskmate_link[{NODE_ID}]: ignoring the invalid features of an info frame"
    ]
    assert "RECEIVED" not in caplog.text


async def test_later_info_replaces_the_features(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.send({**INFO, "features": BOTH})
    assert (await client.receive())["t"] == "info"

    await client.send({**INFO, "features": ["media_player"]})

    assert (await client.receive())["t"] == "info"
    assert hub.session_features == {"media_player"}


async def test_invalid_info_keeps_the_features_of_an_earlier_one(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.send({**INFO, "features": BOTH})
    assert (await client.receive())["t"] == "info"

    await client.send({**INFO, "role": "admin", "features": []})

    await client.sync()
    assert hub.session_features == set(BOTH)


async def test_features_are_cleared_when_the_session_ends(
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Callable[[], Awaitable[None]],
) -> None:
    client = await connect()
    await client.send({**INFO, "features": BOTH})
    assert (await client.receive())["t"] == "info"

    await client.close()
    await wait_until_offline()

    assert hub.session_features == frozenset()


async def test_new_session_does_not_inherit_the_features_of_the_one_it_replaces(
    hub: DeskmateHub, connect: Connect
) -> None:
    first = await connect()
    await first.send({**INFO, "features": BOTH})
    assert (await first.receive())["t"] == "info"

    second = await connect()

    await first.wait_closed()
    await second.sync()
    assert hub.session_features == frozenset()


async def test_diagnostics_show_the_features(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    without_info = await async_get_config_entry_diagnostics(hass, entry)
    await client.send({**INFO, "features": ["notify_tag", "media_player"]})
    assert (await client.receive())["t"] == "info"

    with_info = await async_get_config_entry_diagnostics(hass, entry)

    assert without_info["session_features"] == []
    assert with_info["session_features"] == BOTH
