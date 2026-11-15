"""The `info` session frame (docs/LINK.md, "Session frames").

A client sends it first after the handshake to say what it is and, with
`features`, which optional parts of the protocol it implements. The hub keeps
the values for the session and answers with the integration's version and
the optional parts it understands.
"""

from __future__ import annotations

import re
from typing import Any

from homeassistant.core import HomeAssistant
from homeassistant.loader import async_get_integration

from .const import (
    CLIENT_FEATURES,
    DOMAIN,
    INFO_APP_VERSION_PATTERN,
    INFO_FEATURE_PATTERN,
    INFO_FEATURES_MAX,
    INFO_PLATFORMS,
    INFO_ROLES,
    LINK_CAPS,
)

# re.ASCII: `\d` must not accept digits from other scripts.
_APP_VERSION_RE = re.compile(INFO_APP_VERSION_PATTERN, re.ASCII)
_FEATURE_RE = re.compile(INFO_FEATURE_PATTERN, re.ASCII)


def invalid_info_field(frame: dict[str, Any]) -> str | None:
    """Name of the first field of an `info` frame that is not valid, or None.

    The caller logs the name. The value itself comes from the peer and must
    not reach a log.
    """
    role = frame.get("role")
    if not isinstance(role, str) or role not in INFO_ROLES:
        return "role"
    platform = frame.get("platform")
    if not isinstance(platform, str) or platform not in INFO_PLATFORMS:
        return "platform"
    app = frame.get("app")
    if not isinstance(app, str) or not _APP_VERSION_RE.fullmatch(app):
        return "app"
    return None


def client_features(frame: dict[str, Any]) -> frozenset[str] | None:
    """The known features that an `info` frame lists, or None if the list is invalid.

    A frame without `features` lists none: that is every client older than
    the field. Names this integration does not know are left out, so a newer
    client can list more than it knows about.
    """
    if "features" not in frame:
        return frozenset()
    features = frame["features"]
    if not isinstance(features, list) or len(features) > INFO_FEATURES_MAX:
        return None
    if not all(
        isinstance(name, str) and _FEATURE_RE.fullmatch(name) for name in features
    ):
        return None
    return frozenset(features) & CLIENT_FEATURES


async def async_info_answer(hass: HomeAssistant) -> dict[str, Any]:
    """The hub's answer to a valid `info` frame."""
    # Home Assistant cached the manifest when it loaded the integration, so no
    # file is read here. It does not load a custom integration without a
    # version, so the version is always set.
    integration = await async_get_integration(hass, DOMAIN)
    return {
        "t": "info",
        "integration": str(integration.version),
        "caps": list(LINK_CAPS),
    }
