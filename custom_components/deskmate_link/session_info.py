"""The `info` session frame (docs/LINK.md, "Session frames").

A client sends it first after the handshake to say what it is. The hub keeps
the three values for the session and answers with the integration's version
and the optional frame types it understands.
"""

from __future__ import annotations

import re
from typing import Any

from homeassistant.core import HomeAssistant
from homeassistant.loader import async_get_integration

from .const import (
    DOMAIN,
    INFO_APP_VERSION_PATTERN,
    INFO_PLATFORMS,
    INFO_ROLES,
    LINK_CAPS,
)

# re.ASCII: `\d` must not accept digits from other scripts.
_APP_VERSION_RE = re.compile(INFO_APP_VERSION_PATTERN, re.ASCII)


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
