"""The `media_art` frame: the cover of what the computer plays."""

from __future__ import annotations

import base64
from typing import Any

import pytest
from homeassistant.core import HomeAssistant
from homeassistant.helpers import entity_registry as er
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import DOMAIN
from custom_components.deskmate_link.media_art import (
    MEDIA_ART_MAX_BYTES,
    MediaArt,
    is_media_art_removal,
    parse_media_art,
)

from .link_client import Connect, LinkClient

DEVICE = {"name": "Test PC", "model": "Deskmate for Windows", "sw_version": "1.2.3"}
MEDIA = {"key": "media", "kind": "media_player", "name": "Media"}
PLAYING = {"state": "playing", "title": "Song"}
JPEG = b"\xff\xd8\xff\xe0" + b"cover" * 20
PNG = b"\x89PNG\r\n\x1a\n" + b"cover" * 20
HASH = "0123456789abcdef"


def frame(content: bytes = JPEG, mime: str = "image/jpeg", digest: Any = HASH) -> dict:
    return {
        "t": "media_art",
        "hash": digest,
        "mime": mime,
        "data": base64.b64encode(content).decode("ascii"),
    }


# ── the frame ────────────────────────────────────────────────────


def test_a_jpeg_and_a_png_are_kept() -> None:
    assert parse_media_art(frame()) == MediaArt(JPEG, "image/jpeg", HASH)
    assert parse_media_art(frame(PNG, "image/png", "f" * 64)) == MediaArt(
        PNG, "image/png", "f" * 64
    )


def test_a_picture_of_the_largest_size_is_kept() -> None:
    content = JPEG + b"\0" * (MEDIA_ART_MAX_BYTES - len(JPEG))

    art = parse_media_art(frame(content))

    assert art is not None
    assert len(art.content) == MEDIA_ART_MAX_BYTES


def test_a_larger_picture_is_refused() -> None:
    content = JPEG + b"\0" * (MEDIA_ART_MAX_BYTES - len(JPEG) + 1)

    assert parse_media_art(frame(content)) is None


@pytest.mark.parametrize(
    ("content", "mime"),
    [
        # The type says one thing and the bytes another.
        (PNG, "image/jpeg"),
        (JPEG, "image/png"),
        (b"<svg xmlns='http://www.w3.org/2000/svg'/>", "image/jpeg"),
        (b"<html><script>alert(1)</script></html>", "image/png"),
        (b"", "image/jpeg"),
        # Types that are not accepted at all.
        (JPEG, "image/svg+xml"),
        (JPEG, "text/html"),
        (JPEG, "IMAGE/JPEG"),
        (JPEG, ""),
    ],
)
def test_a_picture_that_is_not_what_it_says_is_refused(
    content: bytes, mime: str
) -> None:
    assert parse_media_art(frame(content, mime)) is None


@pytest.mark.parametrize(
    "digest",
    ["", "0123456789abcde", "0123456789ABCDEF", "g" * 16, "a" * 65, "../etc", 7, None],
)
def test_an_invalid_hash_is_refused(digest: Any) -> None:
    assert parse_media_art(frame(digest=digest)) is None


@pytest.mark.parametrize(
    "data", ["", "not base64!", "PGh0bWw+", "/9j/4AAQ=", 7, ["/9j/"], None]
)
def test_data_that_is_not_base64_of_a_picture_is_refused(data: Any) -> None:
    assert parse_media_art({**frame(), "data": data}) is None


def test_only_a_frame_without_hash_and_data_is_a_removal() -> None:
    assert is_media_art_removal({"t": "media_art"})
    assert is_media_art_removal({"t": "media_art", "hash": None, "data": None})
    assert not is_media_art_removal(frame())
    assert not is_media_art_removal({"t": "media_art", "hash": HASH})


# ── through the hub ──────────────────────────────────────────────


async def playing(hass: HomeAssistant, client: LinkClient) -> None:
    await client.send({"t": "declare", "device": DEVICE, "entities": [MEDIA]})
    await client.send({"t": "state", "s": {"media": PLAYING}})
    await client.sync()
    await hass.async_block_till_done()


async def send(hass: HomeAssistant, client: LinkClient, message: dict) -> None:
    await client.send(message)
    await client.sync()
    await hass.async_block_till_done()


def media_entity(hass: HomeAssistant, entry: MockConfigEntry) -> Any:
    entity_id = er.async_get(hass).async_get_entity_id(
        "media_player", DOMAIN, f"{entry.entry_id}-media"
    )
    assert entity_id is not None
    return hass.data["media_player"].get_entity(entity_id)


async def test_the_picture_reaches_the_media_player(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await playing(hass, client)

    await send(hass, client, frame())

    entity = media_entity(hass, entry)
    assert entity.media_image_hash == HASH
    assert await entity.async_get_media_image() == (JPEG, "image/jpeg")
    state = hass.states.get(entity.entity_id)
    assert HASH in state.attributes["entity_picture"]


async def test_a_new_picture_replaces_the_old_one(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await playing(hass, client)
    await send(hass, client, frame())

    await send(hass, client, frame(PNG, "image/png", "f" * 16))

    entity = media_entity(hass, entry)
    assert entity.media_image_hash == "f" * 16
    assert await entity.async_get_media_image() == (PNG, "image/png")


@pytest.mark.parametrize(
    "message",
    [
        {"t": "media_art"},
        {"t": "media_art", "hash": None, "data": None},
        # A refused picture takes the old one away as well: it belonged to
        # another track.
        {"t": "media_art", "hash": HASH, "mime": "image/jpeg", "data": "PGh0bWw+"},
    ],
)
async def test_a_removal_or_a_refused_picture_leaves_none(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect, message: dict
) -> None:
    client = await connect()
    await playing(hass, client)
    await send(hass, client, frame())

    await send(hass, client, message)

    entity = media_entity(hass, entry)
    assert entity.media_image_hash is None
    assert await entity.async_get_media_image() == (None, None)
    assert "entity_picture" not in hass.states.get(entity.entity_id).attributes


async def test_the_picture_does_not_outlive_the_session(
    hass: HomeAssistant, entry: MockConfigEntry, connect: Connect
) -> None:
    client = await connect()
    await playing(hass, client)
    await send(hass, client, frame())
    entity = media_entity(hass, entry)

    await client.close()
    await hass.async_block_till_done()

    assert await entity.async_get_media_image() == (None, None)
