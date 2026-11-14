"""The `info` session frame: what a client says about itself."""

from __future__ import annotations

import json
import logging
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

import pytest
from homeassistant.core import HomeAssistant
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.diagnostics import (
    async_get_config_entry_diagnostics,
)
from custom_components.deskmate_link.hub import DeskmateHub
from custom_components.deskmate_link.session_info import invalid_info_field

from .link_client import NODE_ID, Connect

MANIFEST = Path(__file__).parents[1] / "custom_components/deskmate_link/manifest.json"
INFO = {"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
ANSWER = {
    "t": "info",
    "integration": json.loads(MANIFEST.read_text(encoding="utf-8"))["version"],
    "caps": ["info"],
}
NO_INFO = (None, None, None)
HUB_LOGGER = "custom_components.deskmate_link.hub"


def session_info(hub: DeskmateHub) -> tuple[str | None, str | None, str | None]:
    return hub.session_role, hub.session_platform, hub.session_app


# ── validation ───────────────────────────────────────────────────


@pytest.mark.parametrize(
    "changes",
    [
        {},
        {"role": "service"},
        {"role": "daemon", "platform": "linux"},
        {"platform": "android_tv"},
        {"app": "10.20.30"},
        {"app": "1.2.3-beta.1"},
        {"app": "1.2.3+build.5"},
        {"app": "1.2.3-" + "a" * 20},
        {"later_addition": "ignored"},
    ],
)
def test_valid_info_frame_is_accepted(changes: dict[str, Any]) -> None:
    assert invalid_info_field({**INFO, **changes}) is None


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("role", "admin"),
        ("role", "App"),
        ("role", " app"),
        ("role", ""),
        ("role", None),
        ("role", 1),
        ("role", ["app"]),
        ("platform", "macos"),
        ("platform", "Windows"),
        ("platform", "android-tv"),
        ("platform", ""),
        ("platform", None),
        ("platform", {"windows": True}),
        ("app", "1.2"),
        ("app", "1.2.3.4"),
        ("app", "v1.2.3"),
        ("app", " 1.2.3"),
        ("app", "1.2.3\n"),
        ("app", "1.2.3-"),
        ("app", "1.2.3-" + "a" * 21),
        ("app", "1.2.3 beta"),
        ("app", "1.2.3-béta"),
        ("app", "١.٢.٣"),
        ("app", ""),
        ("app", None),
        ("app", 123),
    ],
)
def test_invalid_info_field_is_named(field: str, value: Any) -> None:
    assert invalid_info_field({**INFO, field: value}) == field


@pytest.mark.parametrize("field", ["role", "platform", "app"])
def test_missing_info_field_is_named(field: str) -> None:
    frame = {key: value for key, value in INFO.items() if key != field}

    assert invalid_info_field(frame) == field


# ── through the hub ──────────────────────────────────────────────


async def test_valid_info_is_kept_and_answered(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()

    await client.send(INFO)

    assert await client.receive() == ANSWER
    assert session_info(hub) == ("app", "windows", "0.9.0")


@pytest.mark.parametrize("field", ["role", "platform", "app"])
async def test_invalid_info_is_ignored_and_logged_without_its_values(
    hub: DeskmateHub,
    connect: Connect,
    caplog: pytest.LogCaptureFixture,
    field: str,
) -> None:
    client = await connect()

    await client.send({**INFO, field: f"{field}-RECEIVED", "extra": "extra-RECEIVED"})

    # sync() fails if the hub answers the frame.
    await client.sync()
    assert session_info(hub) == NO_INFO
    warnings = [
        record.getMessage()
        for record in caplog.records
        if record.name == HUB_LOGGER and record.levelno == logging.WARNING
    ]
    assert warnings == [
        f"deskmate_link[{NODE_ID}]: ignoring an info frame with an invalid {field}"
    ]
    assert "RECEIVED" not in caplog.text


async def test_invalid_info_keeps_what_an_earlier_valid_one_said(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()
    await client.send(INFO)
    assert await client.receive() == ANSWER

    await client.send({**INFO, "role": "daemon", "app": "not a version"})

    await client.sync()
    assert session_info(hub) == ("app", "windows", "0.9.0")


async def test_session_without_info_gets_no_info_from_the_hub(
    hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()

    # sync() fails if the first frame from the hub is anything but its `pong`.
    await client.sync()
    assert session_info(hub) == NO_INFO


async def test_info_is_cleared_when_the_session_ends(
    hub: DeskmateHub,
    connect: Connect,
    wait_until_offline: Callable[[], Awaitable[None]],
) -> None:
    client = await connect()
    await client.send(INFO)
    assert await client.receive() == ANSWER

    await client.close()
    await wait_until_offline()

    assert session_info(hub) == NO_INFO


async def test_new_session_does_not_inherit_the_info_of_the_one_it_replaces(
    hub: DeskmateHub, connect: Connect
) -> None:
    first = await connect()
    await first.send(INFO)
    assert await first.receive() == ANSWER

    second = await connect()

    await first.wait_closed()
    await second.sync()
    assert session_info(hub) == NO_INFO


async def test_diagnostics_show_the_session_info(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    without_info = await async_get_config_entry_diagnostics(hass, entry)
    await client.send({**INFO, "role": "service", "app": "0.9.0-beta.1"})
    assert await client.receive() == ANSWER

    with_info = await async_get_config_entry_diagnostics(hass, entry)

    for diagnostics, expected in (
        (without_info, NO_INFO),
        (with_info, ("service", "windows", "0.9.0-beta.1")),
    ):
        assert (
            diagnostics["session_role"],
            diagnostics["session_platform"],
            diagnostics["session_app"],
        ) == expected
