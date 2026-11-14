"""Device registry calls that differ between Home Assistant versions.

Older cores keep one device per identifier and let it belong to several
config entries. Newer ones keep one device per config entry: an identifier is
unique within its entry only, the registry-wide lookup is deprecated, and a
device moves to another entry instead of being shared with it. hacs.json
still allows Home Assistant 2024.11, so the integration asks the registry
what it can do.
"""

from __future__ import annotations

import inspect

from homeassistant.core import callback
from homeassistant.helpers import device_registry as dr


@callback
def async_entry_device(
    registry: dr.DeviceRegistry, entry_id: str, identifier: tuple[str, str]
) -> dr.DeviceEntry | None:
    """The device with this identifier, as the config entry sees it."""
    per_entry_lookup = getattr(registry, "async_get_device_by_identifier", None)
    if per_entry_lookup is not None:
        return per_entry_lookup(identifier, entry_id)
    # One device per identifier: every entry sees the same one.
    return registry.async_get_device(identifiers={identifier})


@callback
def async_hand_device_over(
    registry: dr.DeviceRegistry, device_id: str, entry_id: str
) -> None:
    """Give the device to another config entry, so that it outlives its own."""
    update_device = registry.async_update_device
    if "new_config_entry_id" in inspect.signature(update_device).parameters:
        update_device(device_id, new_config_entry_id=entry_id)
        return
    # The device then belongs to both entries and survives the removal of one.
    update_device(device_id, add_config_entry_id=entry_id)
