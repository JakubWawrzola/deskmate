"""The Link handshake, through the WebSocket view."""

from __future__ import annotations

import pytest
from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.deskmate_link.const import (
    CONF_MIN_VERSION,
    CONF_NODE_ID,
    PROTO_V2,
    REJECT_AUTH,
)
from custom_components.deskmate_link.crypto import gen_psk
from custom_components.deskmate_link.hub import DeskmateHub

from .link_client import NODE_ID, Connect, LinkRejectedError


async def test_v2_handshake_with_the_right_key_is_accepted(
    entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    client = await connect()

    # LinkClient.connect has already checked the MAC of the welcome.
    assert client.welcome["v"] == PROTO_V2
    await client.sync()
    assert hub.available
    # The entry that was waiting for pairing is now bound to this computer.
    assert hub.node_id == NODE_ID
    assert entry.data[CONF_NODE_ID] == NODE_ID
    assert entry.data[CONF_MIN_VERSION] == PROTO_V2
    assert entry.unique_id == NODE_ID
    assert entry.title == NODE_ID


# The view closes the socket itself after a `reject`. When the client's closing
# frame then arrives, aiohttp re-arms the heartbeat timer of the socket it has
# just closed. The timer expires on its own and does nothing.
@pytest.mark.parametrize("expected_lingering_timers", [True])
async def test_v2_handshake_with_a_wrong_key_is_rejected(
    entry: MockConfigEntry, hub: DeskmateHub, connect: Connect
) -> None:
    with pytest.raises(LinkRejectedError) as rejected:
        await connect(gen_psk())

    assert rejected.value.reason == REJECT_AUTH
    assert not hub.available
    assert hub.node_id == ""
    assert entry.data[CONF_NODE_ID] == ""
