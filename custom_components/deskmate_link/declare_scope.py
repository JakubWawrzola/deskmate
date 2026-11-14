"""`declare` with a scope (docs/LINK.md, "Session frames").

The Deskmate app declares everything the computer offers. A client that runs
without a signed-in user, such as the Windows service, sends `declare` with
`"scope": "machine"` and lists only what it can serve itself. The hub then
keeps the full declaration it has and limits the session to those keys.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import Any

from .const import DECLARE_SCOPE_MACHINE

# Shown when Home Assistant acts on an entity that the connected client did
# not declare.
NEEDS_APP_ERROR = (
    "Deskmate: this control needs the Deskmate app with a signed-in user, "
    "and the app is not connected right now"
)


def is_machine_declare(frame: dict[str, Any]) -> bool:
    """True for a machine declaration.

    A missing scope and every other value mean a full declaration, which is
    what clients older than the field send.
    """
    scope = frame.get("scope")
    return isinstance(scope, str) and scope == DECLARE_SCOPE_MACHINE


def declared_keys(entities: Iterable[Any]) -> frozenset[str]:
    """The entity keys of a declaration, skipping items that have none."""
    return frozenset(
        entity["key"]
        for entity in entities
        if isinstance(entity, dict)
        and isinstance(entity.get("key"), str)
        and entity["key"]
    )


def merge_machine_declaration(stored: list[Any], declared: list[Any]) -> list[Any]:
    """A machine declaration laid over the machine declaration before it.

    It adds entities and replaces descriptors. An entity that it no longer
    lists stays declared, because only a full declaration removes one.
    """
    keys = declared_keys(declared)
    kept = [
        entity
        for entity in stored
        if isinstance(entity, dict) and entity.get("key") not in keys
    ]
    return [*declared, *kept]
