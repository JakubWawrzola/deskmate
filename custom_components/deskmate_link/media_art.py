"""The picture of what is playing: the `media_art` frame (docs/LINK.md).

The client sends the cover of the current track as a small image, once per
track, and Home Assistant shows it on the media player. The picture comes
from whatever program plays on the computer, so nothing about it is trusted:
its size, its type and its first bytes are checked before it is kept, and it
is only ever handed out as a file of that type.
"""

from __future__ import annotations

import base64
import binascii
import re
from dataclasses import dataclass
from typing import Any

# Largest picture kept, in bytes. A 512 pixel JPEG is well below it.
MEDIA_ART_MAX_BYTES = 256 * 1024
# The length of that many bytes in base64, with its padding.
_MEDIA_ART_MAX_BASE64_CHARS = 4 * ((MEDIA_ART_MAX_BYTES + 2) // 3)
# The content types accepted, each with the bytes such a file starts with.
_SIGNATURES = {
    "image/jpeg": b"\xff\xd8\xff",
    "image/png": b"\x89PNG\r\n\x1a\n",
}
# The client's name for a picture, which tells the frontend when to fetch it
# again.
_HASH_RE = re.compile(r"^[0-9a-f]{16,64}$", re.ASCII)


@dataclass(frozen=True, slots=True)
class MediaArt:
    """A picture that passed every check."""

    content: bytes
    content_type: str
    digest: str


def parse_media_art(frame: dict[str, Any]) -> MediaArt | None:
    """The picture a `media_art` frame carries, or None.

    None stands for no picture: the client took it away with a frame without
    `data`, or the frame failed a check. Either way the picture kept before
    must go, because it belonged to another track.
    """
    digest = frame.get("hash")
    content_type = frame.get("mime")
    data = frame.get("data")
    if not isinstance(digest, str) or _HASH_RE.fullmatch(digest) is None:
        return None
    if not isinstance(content_type, str) or content_type not in _SIGNATURES:
        return None
    if not isinstance(data, str) or not 0 < len(data) <= _MEDIA_ART_MAX_BASE64_CHARS:
        return None
    try:
        content = base64.b64decode(data, validate=True)
    except (binascii.Error, ValueError):
        return None
    if len(content) > MEDIA_ART_MAX_BYTES:
        return None
    if not content.startswith(_SIGNATURES[content_type]):
        return None
    return MediaArt(content=content, content_type=content_type, digest=digest)


def is_media_art_removal(frame: dict[str, Any]) -> bool:
    """True for the frame a client sends when nothing has a picture."""
    return frame.get("data") is None and frame.get("hash") is None
