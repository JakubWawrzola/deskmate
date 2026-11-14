"""What Home Assistant does not ask of the Windows service.

The service holds the connection while nobody is signed in. It has no
desktop to show a notification on and no user whose folders it may open, so
those requests are refused where they enter Home Assistant instead of being
sent to it. The role comes from the session's `info` (docs/LINK.md).
"""

from __future__ import annotations

from homeassistant.exceptions import HomeAssistantError

from .const import ROLE_SERVICE
from .hub import DeskmateHub

SERVICE_ONLY_ERROR = (
    "Deskmate: notifications and files need the Deskmate app with a signed-in "
    "user, and only the Deskmate service is connected right now"
)


def refuse_service_only(hub: DeskmateHub) -> None:
    """Raise when the client connected for this computer is the service."""
    if hub.session_role == ROLE_SERVICE:
        raise HomeAssistantError(SERVICE_ONLY_ERROR)
