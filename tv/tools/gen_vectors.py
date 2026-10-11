"""Deskmate Link v2 test vectors from the REAL crypto.py of the HA integration.

Run from the tv/ directory:
    python -I tools/gen_vectors.py [path/to/deskmate_link/crypto.py]

Writes app/src/test/resources/link_vectors.json. The Kotlin test
(LinkCryptoTest) computes the same values and compares them byte for byte.
Keys are deterministic so the vectors are reproducible.
"""
from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import sys
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey

ROOT = Path(__file__).resolve().parent.parent
# Monorepo: tv/ sits next to custom_components/
DEFAULT_CRYPTO = ROOT.parent / "custom_components" / "deskmate_link" / "crypto.py"


def load_crypto(path: Path):
    # No __pycache__ next to the integration (it would end up in commits)
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("dml_crypto", path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode()


def raw_public(priv: X25519PrivateKey) -> bytes:
    return priv.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )


def main() -> None:
    crypto_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_CRYPTO
    c = load_crypto(crypto_path)

    psk = bytes(range(32))
    cascade = bytes(range(32, 64))
    node = "salon_tv"
    cn = bytes(range(0x10, 0x20))
    sn = bytes(range(0x20, 0x30))
    ts_c = 1760000000
    ts_s = 1760000001
    client_priv_raw = hashlib.sha256(b"client").digest()
    server_priv_raw = hashlib.sha256(b"server").digest()
    client_priv = X25519PrivateKey.from_private_bytes(client_priv_raw)
    server_priv = X25519PrivateKey.from_private_bytes(server_priv_raw)
    client_epk = raw_public(client_priv)
    server_epk = raw_public(server_priv)
    shared = server_priv.exchange(client_priv.public_key())

    vectors: dict = {
        "crypto_py_sha256": hashlib.sha256(crypto_path.read_bytes()).hexdigest(),
        "psk": b64(psk),
        "cascade": b64(cascade),
        "node": node,
        "cn": b64(cn),
        "sn": b64(sn),
        "ts_c": ts_c,
        "ts_s": ts_s,
        "client_priv": b64(client_priv_raw),
        "server_priv": b64(server_priv_raw),
        "client_epk": b64(client_epk),
        "server_epk": b64(server_epk),
        "shared": b64(shared),
    }

    for casc_on in (False, True):
        tag = "casc" if casc_on else "plain"
        hello = c.v2_hello_bytes(node, cn, ts_c, client_epk, casc_on)
        welcome = c.v2_welcome_bytes(hello, sn, ts_s, server_epk)
        k_c2s, k_s2c = c.v2_session_keys(b64(psk), shared, hello, welcome)
        casc_c2s = casc_s2c = None
        if casc_on:
            casc_c2s, casc_s2c = c.v2_cascade_keys(b64(cascade), shared, hello, welcome)
        # Ramka c2s (klient -> HA) i s2c (HA -> klient) z prawdziwego FrameCodec
        tx = c.FrameCodec(k_c2s, c_dir(c, "c2s"), node, "c2s", casc_c2s, 2)
        rx_srv = c.FrameCodec(k_s2c, c_dir(c, "s2c"), node, "s2c", casc_s2c, 2)
        payload_c2s = {"t": "state", "s": {"app": "Netflix", "media_title": "Zażółć gęślą jaźń"}}
        payload_s2c = {"t": "tv", "id": 7, "op": "screen", "max": 60}
        frame_c2s = tx.encrypt(payload_c2s)
        frame_s2c = rx_srv.encrypt(payload_s2c)
        vectors[tag] = {
            "hello_bytes": hello.hex(),
            "hello_mac": c.v2_mac(b64(psk), hello),
            "welcome_bytes": welcome.hex(),
            "welcome_mac": c.v2_mac(b64(psk), welcome),
            "k_c2s": k_c2s.hex(),
            "k_s2c": k_s2c.hex(),
            "casc_c2s": casc_c2s.hex() if casc_c2s else None,
            "casc_s2c": casc_s2c.hex() if casc_s2c else None,
            "c2s_plain": json.dumps(payload_c2s),
            "c2s_frame": frame_c2s,
            "s2c_plain": json.dumps(payload_s2c),
            "s2c_frame": frame_s2c,
        }

    out = ROOT / "app" / "src" / "test" / "resources" / "link_vectors.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(vectors, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"written {out}")


def c_dir(c, name: str) -> bytes:
    """Direction nonce prefix - the same bytes as in the integration's const.py."""
    return b"\x01\x00\x00\x00" if name == "c2s" else b"\x02\x00\x00\x00"


if __name__ == "__main__":
    main()
