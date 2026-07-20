"""Generate public Deskmate Link v1 compatibility vectors with the HA implementation."""

from __future__ import annotations

import argparse
import base64
import importlib.util
import json
from pathlib import Path


def load_ha_crypto(home_assistant: Path):
    source = home_assistant / "domos" / "custom_components" / "deskmate_link" / "crypto.py"
    spec = importlib.util.spec_from_file_location("deskmate_link_ha_crypto", source)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {source}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--home-assistant", required=True, type=Path)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("src-tauri/tests/fixtures/deskmate_link_v1.json"),
    )
    args = parser.parse_args()
    crypto = load_ha_crypto(args.home_assistant.resolve())

    psk = bytes(range(32))
    cn = bytes(range(0x10, 0x20))
    sn = bytes(range(0x20, 0x30))
    psk_b64 = base64.b64encode(psk).decode()
    cn_b64 = base64.b64encode(cn).decode()
    sn_b64 = base64.b64encode(sn).decode()
    node = "compat_node"
    hello_ts = 1_750_000_000
    welcome_ts = 1_750_000_001
    c2s, s2c = crypto.derive_session_keys(psk_b64, cn_b64, sn_b64)
    payload = {"t": "state", "s": {"ac_power": True, "volume": 37, "cpu": "12.5"}}
    frame = crypto.FrameCodec(c2s, b"\x01\x00\x00\x00", node, "c2s").encrypt(payload)
    vector = {
        "protocol": "dml1",
        "psk_b64": psk_b64,
        "node": node,
        "cn_b64": cn_b64,
        "sn_b64": sn_b64,
        "hello_ts": hello_ts,
        "welcome_ts": welcome_ts,
        "hello_mac_b64": crypto.hs_mac(psk_b64, "hello", node, cn_b64, str(hello_ts)),
        "welcome_mac_b64": crypto.hs_mac(
            psk_b64, "welcome", node, cn_b64, sn_b64, str(welcome_ts)
        ),
        "c2s_key_b64": base64.b64encode(c2s).decode(),
        "s2c_key_b64": base64.b64encode(s2c).decode(),
        "payload": payload,
        "python_c2s_frame": frame,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(vector, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
