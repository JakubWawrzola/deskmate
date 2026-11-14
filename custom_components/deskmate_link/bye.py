"""The `bye` session frame (docs/LINK.md, "Session frames").

A client sends it just before it closes the socket, to say why. The hub
passes the reason on to `Presence`, which decides what the end of the session
means for the computer's availability.
"""

from __future__ import annotations

from typing import Any

from .const import BYE_REASONS


def invalid_bye_field(frame: dict[str, Any]) -> str | None:
    """Name of the field of a `bye` frame that is not valid, or None.

    The caller logs the name. The value itself comes from the peer and must
    not reach a log.
    """
    reason = frame.get("reason")
    if not isinstance(reason, str) or reason not in BYE_REASONS:
        return "reason"
    return None
