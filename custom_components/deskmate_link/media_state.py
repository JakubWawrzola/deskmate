"""State and commands of the `media_player` entity kind (docs/LINK.md).

The client reports what is playing as one object under the entity's key. Each
field is checked here before it reaches the entity, and a field that fails is
dropped. Commands go back as the `value` of a `set` command.
"""

from __future__ import annotations

import math
import unicodedata
from dataclasses import dataclass
from typing import Any

from homeassistant.exceptions import HomeAssistantError

MEDIA_STATES = frozenset({"playing", "paused", "idle", "off"})
MEDIA_TEXT_MAX_CHARS = 256
MEDIA_OPS = frozenset(
    {"play_pause", "play", "pause", "next", "previous", "set_volume", "mute"}
)
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


def parse_media_state(value: Any) -> MediaState | None:
    """The state the client reported, or None when it is no valid state object."""
    if not isinstance(value, dict):
        return None
    state = value.get("state")
    if not isinstance(state, str) or state not in MEDIA_STATES:
        return None
    muted = value.get("muted")
    return MediaState(
        state=state,
        title=_text(value.get("title")),
        artist=_text(value.get("artist")),
        app=_text(value.get("app")),
        volume=_volume(value.get("volume")),
        muted=muted if isinstance(muted, bool) else None,
    )


def media_command(
    op: str, *, volume: float | None = None, muted: bool | None = None
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
    return {"op": op}


def _text(value: Any) -> str | None:
    if not isinstance(value, str) or not value or len(value) > MEDIA_TEXT_MAX_CHARS:
        return None
    if any(unicodedata.category(char) in _REFUSED_CATEGORIES for char in value):
        return None
    return value


def _volume(value: Any) -> float | None:
    # bool is an int in Python, and json.loads accepts NaN and Infinity.
    if not isinstance(value, int | float) or isinstance(value, bool):
        return None
    if not math.isfinite(value) or not 0 <= value <= 1:
        return None
    return float(value)
