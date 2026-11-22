"""Media players of Deskmate Link: what the computer is playing (docs/LINK.md)."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from homeassistant.components.media_player import (
    MediaPlayerEntity,
    MediaPlayerEntityFeature,
    MediaPlayerState,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .entity import DeskmateEntity, setup_dynamic_platform
from .media_state import MediaState, media_command, parse_media_state

_STATES = {
    "playing": MediaPlayerState.PLAYING,
    "paused": MediaPlayerState.PAUSED,
    "idle": MediaPlayerState.IDLE,
    "off": MediaPlayerState.OFF,
}


# Play/pause needs no flag of its own: Home Assistant offers it with PLAY and
# PAUSE, and the entity sends it as one command.
_FEATURES = (
    MediaPlayerEntityFeature.PLAY
    | MediaPlayerEntityFeature.PAUSE
    | MediaPlayerEntityFeature.NEXT_TRACK
    | MediaPlayerEntityFeature.PREVIOUS_TRACK
    | MediaPlayerEntityFeature.VOLUME_SET
    | MediaPlayerEntityFeature.VOLUME_MUTE
)


class DeskmateMediaPlayer(DeskmateEntity, MediaPlayerEntity):
    @property
    def _media(self) -> MediaState | None:
        return parse_media_state(self._value)

    @property
    def supported_features(self) -> MediaPlayerEntityFeature:
        # Seeking is offered only while the client reports a timeline. A
        # Deskmate older than 0.9.5 never does, so it is never sent a `seek`
        # it would refuse.
        media = self._media
        if media is not None and media.seekable:
            return _FEATURES | MediaPlayerEntityFeature.SEEK
        return _FEATURES

    @property
    def state(self) -> MediaPlayerState | None:
        media = self._media
        return None if media is None else _STATES[media.state]

    @property
    def media_title(self) -> str | None:
        media = self._media
        return None if media is None else media.title

    @property
    def media_artist(self) -> str | None:
        media = self._media
        return None if media is None else media.artist

    @property
    def media_album_name(self) -> str | None:
        media = self._media
        return None if media is None else media.album

    @property
    def media_duration(self) -> int | None:
        media = self._media
        if media is None or media.duration is None:
            return None
        return round(media.duration)

    @property
    def media_position(self) -> int | None:
        media = self._media
        if media is None or media.position is None:
            return None
        return round(media.position)

    @property
    def media_position_updated_at(self) -> datetime | None:
        media = self._media
        return None if media is None else media.position_at

    @property
    def media_image_hash(self) -> str | None:
        # Home Assistant fetches the picture again when this changes.
        art = self.hub.media_art
        return None if art is None or self._media is None else art.digest

    async def async_get_media_image(self) -> tuple[bytes | None, str | None]:
        art = self.hub.media_art
        if art is None or self._media is None:
            return None, None
        return art.content, art.content_type

    @property
    def app_name(self) -> str | None:
        media = self._media
        return None if media is None else media.app

    @property
    def volume_level(self) -> float | None:
        media = self._media
        return None if media is None else media.volume

    @property
    def is_volume_muted(self) -> bool | None:
        media = self._media
        return None if media is None else media.muted

    async def async_media_play(self) -> None:
        await self._send(media_command("play"))

    async def async_media_pause(self) -> None:
        await self._send(media_command("pause"))

    async def async_media_play_pause(self) -> None:
        # The computer knows better than the last state report what plays.
        await self._send(media_command("play_pause"))

    async def async_media_next_track(self) -> None:
        await self._send(media_command("next"))

    async def async_media_previous_track(self) -> None:
        await self._send(media_command("previous"))

    async def async_set_volume_level(self, volume: float) -> None:
        await self._send(media_command("set_volume", volume=volume))

    async def async_mute_volume(self, mute: bool) -> None:
        await self._send(media_command("mute", muted=mute))

    async def async_media_seek(self, position: float) -> None:
        await self._send(media_command("seek", position=position))

    async def _send(self, value: dict[str, Any]) -> None:
        await self.hub.async_command(self.key, "set", value)


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    setup_dynamic_platform(
        hass, entry, async_add_entities, "media_player", DeskmateMediaPlayer
    )
