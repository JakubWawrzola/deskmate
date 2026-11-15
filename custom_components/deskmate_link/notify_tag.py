"""Notification tags and priorities, and the `notify_clear` frame (docs/LINK.md).

A client that implements them lists the feature `notify_tag` in its `info`.
Only such a session gets `tag`, `priority` and `notify_clear`. An older client
gets the notification without the two fields, and clearing fails in Home
Assistant before anything is sent.
"""

from __future__ import annotations

import re
from typing import Any

import voluptuous as vol
from homeassistant.exceptions import HomeAssistantError

from .const import FEATURE_NOTIFY_TAG

NOTIFY_TAG_PATTERN = r"^[A-Za-z0-9_-]{1,64}$"
NOTIFY_PRIORITIES = ("normal", "high", "urgent")
TOO_OLD_TO_CLEAR_ERROR = (
    "Deskmate: this computer's Deskmate is too old to clear notifications. "
    "Update Deskmate on it."
)
_TAG_RE = re.compile(NOTIFY_TAG_PATTERN, re.ASCII)


def is_valid_tag(tag: Any) -> bool:
    return isinstance(tag, str) and _TAG_RE.fullmatch(tag) is not None


def validate_tag(value: Any) -> str:
    """Voluptuous validator for the `tag` field of the actions."""
    if not is_valid_tag(value):
        raise vol.Invalid("tag must be 1 to 64 letters, digits, underscores or hyphens")
    return value


def notify_frame(
    features: frozenset[str],
    title: str,
    message: str,
    image: str | None = None,
    actions: list[dict] | None = None,
    tag: str | None = None,
    priority: str | None = None,
) -> dict[str, Any]:
    """The `notify` frame for a session that listed `features`."""
    if tag is not None and not is_valid_tag(tag):
        raise HomeAssistantError("Deskmate: invalid notification tag")
    if priority is not None and priority not in NOTIFY_PRIORITIES:
        raise HomeAssistantError("Deskmate: invalid notification priority")
    payload: dict[str, Any] = {"t": "notify", "title": title, "message": message}
    if image:
        payload["image"] = image
    if actions:
        payload["actions"] = actions
    if FEATURE_NOTIFY_TAG not in features:
        # An older client would show the notification as an untagged one
        # anyway, so the fields are left out rather than refused.
        return payload
    if tag is not None:
        payload["tag"] = tag
    if priority is not None:
        payload["priority"] = priority
    return payload


def notify_clear_frame(tag: str) -> dict[str, Any]:
    if not is_valid_tag(tag):
        raise HomeAssistantError("Deskmate: invalid notification tag")
    return {"t": "notify_clear", "tag": tag}


def require_notify_tag(features: frozenset[str]) -> None:
    """Raise when the session cannot clear a notification by its tag."""
    if FEATURE_NOTIFY_TAG not in features:
        raise HomeAssistantError(TOO_OLD_TO_CLEAR_ERROR)
