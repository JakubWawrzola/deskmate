"""State and commands of the `media_player` entity kind (docs/LINK.md).

The client reports what is playing as one object under the entity's key. Each
field is checked here before it reaches the entity, and a field that fails is
dropped. Commands go back as the `value` of a `set` command.
"""

from __future__ import annotations

import math
import unicodedata
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

from homeassistant.exceptions import HomeAssistantError

MEDIA_STATES = frozenset({"playing", "paused", "idle", "off"})
MEDIA_TEXT_MAX_CHARS = 256
MEDIA_OPS = frozenset(
    {"play_pause", "play", "pause", "next", "previous", "set_volume", "mute", "seek"}
)
# Longest track and furthest position, in seconds. A week covers every
# recording; anything longer is a client that sends nonsense.
MEDIA_MAX_SECONDS = 7 * 24 * 3600
# `position_at` is Unix time. Outside these years it is no clock reading.
_POSITION_AT_MIN = datetime(2020, 1, 1, tzinfo=UTC).timestamp()
_POSITION_AT_MAX = datetime(2100, 1, 1, tzinfo=UTC).timestamp()
# Control characters, and lone surrogates, which no JSON encoder writes back.
_REFUSED_CATEGORIES = frozenset({"Cc", "Cs"})


@dataclass(frozen=True, slots=True)
class MediaState:
    """A validated state object. A field that did not validate is None."""

    state: str
    title: str | None
    artist: str | None
    app: str | None
    volume: float | None
    muted: bool | None
    album: str | None = None
    # Seconds. `position` is where the track was at `position_at`; the two
    # are kept or dropped together, because one is useless without the other.
    duration: float | None = None
    position: float | None = None
    position_at: datetime | None = None

    @property
    def seekable(self) -> bool:
        """True when the client reports a timeline to move along."""
        return self.duration is not None and self.position is not None


def parse_media_state(value: Any) -> MediaState | None:
    """The state the client reported, or None when it is no valid state object."""
    if not isinstance(value, dict):
        return None
    state = value.get("state")
    if not isinstance(state, str) or state not in MEDIA_STATES:
        return None
    muted = value.get("muted")
    duration = _seconds(value.get("duration"))
    position = _seconds(value.get("position"))
    position_at = _position_at(value.get("position_at"))
    if (
        position is None
        or position_at is None
        or (duration is not None and position > duration)
    ):
        position = position_at = None
    return MediaState(
        state=state,
        title=_text(value.get("title")),
        artist=_text(value.get("artist")),
        app=_text(value.get("app")),
        volume=_volume(value.get("volume")),
        muted=muted if isinstance(muted, bool) else None,
        album=_text(value.get("album")),
        duration=duration,
        position=position,
        position_at=position_at,
    )


def media_command(
    op: str,
    *,
    volume: float | None = None,
    muted: bool | None = None,
    position: float | None = None,
) -> dict[str, Any]:
    """The `value` of a `set` command for a media player."""
    if op not in MEDIA_OPS:
        raise HomeAssistantError(f"Deskmate: unknown media command {op!r}")
    if op == "set_volume":
        checked = _volume(volume)
        if checked is None:
            raise HomeAssistantError("Deskmate: the volume must be between 0 and 1")
        return {"op": op, "volume": checked}
    if op == "mute":
        if not isinstance(muted, bool):
            raise HomeAssistantError("Deskmate: mute needs true or false")
        return {"op": op, "muted": muted}
    if op == "seek":
        checked = _seconds(position)
        if checked is None:
            raise HomeAssistantError(
                "Deskmate: the position must be a number of seconds, zero or more"
            )
        return {"op": op, "position": checked}
    return {"op": op}


def _text(value: Any) -> str | None:
    if not isinstance(value, str) or not value or len(value) > MEDIA_TEXT_MAX_CHARS:
        return None
    if any(unicodedata.category(char) in _REFUSED_CATEGORIES for char in value):
        return None
    return value


def _number(value: Any) -> float | None:
    # bool is an int in Python, and json.loads accepts NaN and Infinity.
    if not isinstance(value, int | float) or isinstance(value, bool):
        return None
    try:
        number = float(value)
    except OverflowError:
        # An int too large for a float, which json.loads accepts.
        return None
    return number if math.isfinite(number) else None


def _volume(value: Any) -> float | None:
    number = _number(value)
    if number is None or not 0 <= number <= 1:
        return None
    return number


def _seconds(value: Any) -> float | None:
    number = _number(value)
    if number is None or not 0 <= number <= MEDIA_MAX_SECONDS:
        return None
    return number


def _position_at(value: Any) -> datetime | None:
    number = _number(value)
    if number is None or not _POSITION_AT_MIN <= number <= _POSITION_AT_MAX:
        return None
    return datetime.fromtimestamp(number, UTC)
