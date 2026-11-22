"""The `media_player` entity kind: its state object and its commands."""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from datetime import UTC, datetime
from typing import Any

import pytest
import voluptuous as vol
from homeassistant.components.media_player import MediaPlayerEntityFeature
from homeassistant.const import STATE_UNAVAILABLE, STATE_UNKNOWN
from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers import entity_registry as er
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import DOMAIN
from custom_components.deskmate_link.media_state import (
    MEDIA_MAX_SECONDS,
    MEDIA_TEXT_MAX_CHARS,
    MediaState,
    media_command,
    parse_media_state,
)

from .link_client import Connect, LinkClient

DEVICE = {"name": "Test PC", "model": "Deskmate for Windows", "sw_version": "1.2.3"}
MEDIA = {"key": "media", "kind": "media_player", "name": "Media"}
PLAYING = {
    "state": "playing",
    "title": "Song",
    "artist": "Band",
    "app": "Spotify",
    "volume": 0.5,
    "muted": False,
}
SUPPORTED = (
    MediaPlayerEntityFeature.PLAY
    | MediaPlayerEntityFeature.PAUSE
    | MediaPlayerEntityFeature.NEXT_TRACK
    | MediaPlayerEntityFeature.PREVIOUS_TRACK
    | MediaPlayerEntityFeature.VOLUME_SET
    | MediaPlayerEntityFeature.VOLUME_MUTE
)


# ── the state object ─────────────────────────────────────────────


def test_full_state_object_is_read() -> None:
    assert parse_media_state(PLAYING) == MediaState(
        state="playing",
        title="Song",
        artist="Band",
        app="Spotify",
        volume=0.5,
        muted=False,
    )


@pytest.mark.parametrize("state", ["playing", "paused", "idle", "off"])
def test_state_alone_is_enough(state: str) -> None:
    assert parse_media_state({"state": state}) == MediaState(
        state=state, title=None, artist=None, app=None, volume=None, muted=None
    )


def test_null_fields_stay_empty() -> None:
    nulls = {"title": None, "artist": None, "app": None, "volume": None, "muted": None}

    parsed = parse_media_state({"state": "idle", **nulls})

    assert parsed == MediaState("idle", None, None, None, None, None)


@pytest.mark.parametrize(
    "value",
    [
        None,
        "playing",
        ["playing"],
        {},
        {"title": "Song"},
        {"state": "Playing"},
        {"state": "stopped"},
        {"state": " playing"},
        {"state": None},
        {"state": 1},
    ],
)
def test_object_without_a_valid_state_is_dropped(value: Any) -> None:
    assert parse_media_state(value) is None


@pytest.mark.parametrize("field", ["title", "artist", "app"])
@pytest.mark.parametrize(
    "value",
    [
        "",
        "x" * (MEDIA_TEXT_MAX_CHARS + 1),
        "line\nbreak",
        "nul\x00",
        "del\x7f",
        "lone \ud800 surrogate",
        1,
        True,
        ["Song"],
        {"text": "Song"},
    ],
)
def test_invalid_text_field_is_dropped_alone(field: str, value: Any) -> None:
    parsed = parse_media_state({**PLAYING, field: value})

    assert parsed is not None
    assert getattr(parsed, field) is None
    assert parsed.state == "playing"
    assert parsed.volume == 0.5


@pytest.mark.parametrize("field", ["title", "artist", "app"])
@pytest.mark.parametrize("value", ["x" * MEDIA_TEXT_MAX_CHARS, "Zażółć 🎵 ♫", "a"])
def test_text_up_to_the_limit_is_kept(field: str, value: str) -> None:
    parsed = parse_media_state({**PLAYING, field: value})

    assert parsed is not None
    assert getattr(parsed, field) == value


@pytest.mark.parametrize("value", [0, 1, 0.0, 1.0, 0.25])
def test_volume_in_range_is_kept(value: float) -> None:
    parsed = parse_media_state({**PLAYING, "volume": value})

    assert parsed is not None
    assert parsed.volume == value
    assert isinstance(parsed.volume, float)


@pytest.mark.parametrize(
    "value",
    [
        -0.01,
        1.01,
        2,
        True,
        False,
        "0.5",
        float("nan"),
        float("inf"),
        [0.5],
        # json.loads makes an int of up to 4300 digits, too large for a float.
        pytest.param(10**400, id="huge_int"),
        pytest.param(-(10**400), id="huge_negative_int"),
    ],
)
def test_invalid_volume_is_dropped_alone(value: Any) -> None:
    parsed = parse_media_state({**PLAYING, "volume": value})

    assert parsed is not None
    assert parsed.volume is None
    assert parsed.title == "Song"


@pytest.mark.parametrize("value", ["true", 1, 0, "false"])
def test_invalid_muted_is_dropped_alone(value: Any) -> None:
    parsed = parse_media_state({**PLAYING, "muted": value})

    assert parsed is not None
    assert parsed.muted is None


def test_unknown_fields_are_ignored() -> None:
    assert parse_media_state({**PLAYING, "later": "x"}) == parse_media_state(PLAYING)


# ── commands ─────────────────────────────────────────────────────


@pytest.mark.parametrize("op", ["play_pause", "play", "pause", "next", "previous"])
def test_command_without_arguments(op: str) -> None:
    assert media_command(op) == {"op": op}


@pytest.mark.parametrize("volume", [0, 0.5, 1])
def test_set_volume_carries_the_volume(volume: float) -> None:
    assert media_command("set_volume", volume=volume) == {
        "op": "set_volume",
        "volume": float(volume),
    }


@pytest.mark.parametrize("muted", [True, False])
def test_mute_carries_the_flag(muted: bool) -> None:
    assert media_command("mute", muted=muted) == {"op": "mute", "muted": muted}


@pytest.mark.parametrize(
    ("op", "arguments"),
    [
        ("stop", {}),
        ("Play", {}),
        ("", {}),
        ("set_volume", {}),
        ("set_volume", {"volume": 1.5}),
        ("set_volume", {"volume": -0.1}),
        ("set_volume", {"volume": float("nan")}),
        ("set_volume", {"volume": True}),
        ("mute", {}),
        ("mute", {"muted": 1}),
        ("mute", {"muted": "true"}),
    ],
)
def test_invalid_command_is_refused(op: str, arguments: dict[str, Any]) -> None:
    with pytest.raises(HomeAssistantError):
        media_command(op, **arguments)


# ── through the hub ──────────────────────────────────────────────


async def declare_media(hass: HomeAssistant, client: LinkClient) -> None:
    await client.send({"t": "declare", "device": DEVICE, "entities": [MEDIA]})
    await client.sync()
    await hass.async_block_till_done()


async def report(hass: HomeAssistant, client: LinkClient, value: Any) -> None:
    await client.send({"t": "state", "s": {"media": value}})
    await client.sync()
    await hass.async_block_till_done()


def media_entity_id(hass: HomeAssistant, entry: MockConfigEntry) -> str:
    entity_id = er.async_get(hass).async_get_entity_id(
        "media_player", DOMAIN, f"{entry.entry_id}-media"
    )
    assert entity_id is not None
    return entity_id


async def test_declared_media_player_exists_with_its_features(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()

    await declare_media(hass, client)

    state = hass.states.get(media_entity_id(hass, entry))
    assert state is not None
    assert state.state == STATE_UNKNOWN
    assert state.attributes["supported_features"] == SUPPORTED
    assert state.attributes["friendly_name"] == "Test PC Media"


async def test_reported_state_reaches_the_entity(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)

    await report(hass, client, PLAYING)

    state = hass.states.get(media_entity_id(hass, entry))
    assert state.state == "playing"
    assert state.attributes["media_title"] == "Song"
    assert state.attributes["media_artist"] == "Band"
    assert state.attributes["app_name"] == "Spotify"
    assert state.attributes["volume_level"] == 0.5
    assert state.attributes["is_volume_muted"] is False


async def test_invalid_field_is_left_out_of_the_entity(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)

    await report(hass, client, {**PLAYING, "title": "x" * 300, "volume": 7})

    state = hass.states.get(media_entity_id(hass, entry))
    assert state.state == "playing"
    assert "media_title" not in state.attributes
    assert "volume_level" not in state.attributes
    assert state.attributes["media_artist"] == "Band"


async def test_invalid_state_object_makes_the_state_unknown(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)
    await report(hass, client, PLAYING)

    await report(hass, client, {**PLAYING, "state": "stopped"})

    state = hass.states.get(media_entity_id(hass, entry))
    assert state.state == STATE_UNKNOWN
    assert "media_title" not in state.attributes


async def test_media_player_becomes_unavailable_when_the_session_ends(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    wait_until_offline: Callable[[], Awaitable[None]],
) -> None:
    client = await connect()
    await declare_media(hass, client)
    await report(hass, client, PLAYING)

    await client.close()
    await wait_until_offline()

    assert hass.states.get(media_entity_id(hass, entry)).state == STATE_UNAVAILABLE


@pytest.mark.parametrize(
    ("service", "data", "value"),
    [
        ("media_play", {}, {"op": "play"}),
        ("media_pause", {}, {"op": "pause"}),
        ("media_play_pause", {}, {"op": "play_pause"}),
        ("media_next_track", {}, {"op": "next"}),
        ("media_previous_track", {}, {"op": "previous"}),
        ("volume_set", {"volume_level": 0.3}, {"op": "set_volume", "volume": 0.3}),
        ("volume_mute", {"is_volume_muted": True}, {"op": "mute", "muted": True}),
        ("volume_mute", {"is_volume_muted": False}, {"op": "mute", "muted": False}),
    ],
)
async def test_service_sends_its_command_and_waits_for_the_ack(
    hass: HomeAssistant,
    entry: MockConfigEntry,
    connect: Connect,
    service: str,
    data: dict[str, Any],
    value: dict[str, Any],
) -> None:
    client = await connect()
    await declare_media(hass, client)
    await report(hass, client, PLAYING)

    call = hass.async_create_task(
        hass.services.async_call(
            "media_player",
            service,
            {"entity_id": media_entity_id(hass, entry), **data},
            blocking=True,
        )
    )
    command = await client.receive()
    assert not call.done()
    await client.send({"t": "ack", "id": command["id"], "ok": True})
    await call

    assert command == {
        "t": "cmd",
        "id": command["id"],
        "key": "media",
        "action": "set",
        "value": value,
    }


async def test_refused_command_fails_the_service(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)
    await report(hass, client, PLAYING)

    call = hass.async_create_task(
        hass.services.async_call(
            "media_player",
            "media_next_track",
            {"entity_id": media_entity_id(hass, entry)},
            blocking=True,
        )
    )
    command = await client.receive()
    await client.send(
        {"t": "ack", "id": command["id"], "ok": False, "error": "nothing plays"}
    )

    with pytest.raises(RuntimeError, match="nothing plays"):
        await call


@pytest.mark.parametrize("volume", [1.5, -0.2])
async def test_volume_out_of_range_sends_nothing(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect, volume: float
) -> None:
    client = await connect()
    await declare_media(hass, client)
    await report(hass, client, PLAYING)

    with pytest.raises(vol.Invalid):
        await hass.services.async_call(
            "media_player",
            "volume_set",
            {"entity_id": media_entity_id(hass, entry), "volume_level": volume},
            blocking=True,
        )

    # sync() fails if the hub sent a command anyway.
    await client.sync()


# ── album, duration, position and seeking (0.9.5) ────────────────

# 2026-11-22 12:00:00 UTC
POSITION_AT = 1_795_348_800
TIMELINE = {
    **PLAYING,
    "album": "Record",
    "duration": 241.5,
    "position": 63.2,
    "position_at": POSITION_AT,
}


def test_timeline_fields_are_read() -> None:
    media = parse_media_state(TIMELINE)

    assert media is not None
    assert media.album == "Record"
    assert media.duration == 241.5
    assert media.position == 63.2
    assert media.position_at == datetime.fromtimestamp(POSITION_AT, UTC)
    assert media.seekable


def test_state_without_timeline_fields_is_what_it_was() -> None:
    media = parse_media_state(PLAYING)

    assert media is not None
    assert (media.album, media.duration, media.position, media.position_at) == (
        None,
        None,
        None,
        None,
    )
    assert not media.seekable


@pytest.mark.parametrize(
    "value",
    [-1, MEDIA_MAX_SECONDS + 1, float("nan"), float("inf"), "241", True, None, [241]],
)
def test_invalid_duration_is_dropped_alone(value: Any) -> None:
    media = parse_media_state({**TIMELINE, "duration": value})

    assert media is not None
    assert media.duration is None
    assert media.title == "Song"
    # Without a length there is nothing to seek along.
    assert not media.seekable


@pytest.mark.parametrize(
    "fields",
    [
        {"position": -0.5},
        {"position": "63"},
        {"position": True},
        {"position": float("nan")},
        # Past the end of the track.
        {"position": 300},
        # A position without the time it was read at cannot be extrapolated.
        {"position_at": None},
        {"position_at": "2026-11-22T12:00:00Z"},
        {"position_at": 0},
        {"position_at": 99_999_999_999},
        {"position_at": float("inf")},
    ],
)
def test_position_and_its_time_are_dropped_together(fields: dict[str, Any]) -> None:
    media = parse_media_state({**TIMELINE, **fields})

    assert media is not None
    assert media.position is None
    assert media.position_at is None
    assert media.duration == 241.5
    assert not media.seekable


def test_position_without_a_duration_is_kept_but_not_seekable() -> None:
    media = parse_media_state({**TIMELINE, "duration": None})

    assert media is not None
    assert media.position == 63.2
    assert not media.seekable


@pytest.mark.parametrize("position", [0, 12.5, MEDIA_MAX_SECONDS])
def test_seek_carries_the_position(position: float) -> None:
    assert media_command("seek", position=position) == {
        "op": "seek",
        "position": float(position),
    }


@pytest.mark.parametrize(
    "arguments",
    [
        {},
        {"position": -1},
        {"position": MEDIA_MAX_SECONDS + 1},
        {"position": float("nan")},
        {"position": True},
        {"position": "12"},
    ],
)
def test_invalid_seek_is_refused(arguments: dict[str, Any]) -> None:
    with pytest.raises(HomeAssistantError):
        media_command("seek", **arguments)


async def test_timeline_reaches_the_entity_and_adds_seeking(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)

    await report(hass, client, TIMELINE)

    state = hass.states.get(media_entity_id(hass, entry))
    assert state.attributes["media_album_name"] == "Record"
    assert state.attributes["media_duration"] == 242
    assert state.attributes["media_position"] == 63
    assert state.attributes["media_position_updated_at"] == datetime.fromtimestamp(
        POSITION_AT, UTC
    )
    assert (
        state.attributes["supported_features"]
        == SUPPORTED | MediaPlayerEntityFeature.SEEK
    )


async def test_seeking_is_not_offered_without_a_timeline(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)

    # What a Deskmate older than 0.9.5 reports.
    await report(hass, client, PLAYING)

    state = hass.states.get(media_entity_id(hass, entry))
    assert state.attributes["supported_features"] == SUPPORTED
    assert "media_duration" not in state.attributes
    assert "media_position" not in state.attributes


async def test_seek_service_sends_the_position(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await declare_media(hass, client)
    await report(hass, client, TIMELINE)

    call = hass.async_create_task(
        hass.services.async_call(
            "media_player",
            "media_seek",
            {"entity_id": media_entity_id(hass, entry), "seek_position": 120},
            blocking=True,
        )
    )
    command = await client.receive()
    await client.send({"t": "ack", "id": command["id"], "ok": True})
    await call

    assert command["value"] == {"op": "seek", "position": 120.0}
