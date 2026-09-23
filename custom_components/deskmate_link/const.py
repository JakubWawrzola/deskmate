"""Stale integracji Deskmate Link."""

DOMAIN = "deskmate_link"

CONF_NODE_ID = "node_id"
CONF_KEY = "key"  # PSK, base64 32 B
CONF_CASCADE_KEY = "cascade_key"  # opcjonalna druga warstwa, base64 32 B
# Najnizsza akceptowana wersja protokolu. Nowe wpisy startuja od 2; stare
# (sprzed 0.6.0) maja 1 i same przechodza na 2 po pierwszym udanym handshake'u
# v2 - od tej chwili v1 jest odrzucane (brak cichego downgrade'u).
CONF_MIN_VERSION = "min_version"

WS_URL = "/api/deskmate_link/ws"

PROTO_V1 = 1
PROTO_V2 = 2
SUPPORTED_VERSIONS = (PROTO_V1, PROTO_V2)
# Nazwa node'a z Deskmate to zsanityzowany hostname: [a-z0-9_]
NODE_ID_PATTERN = r"^[a-z0-9_]{1,64}$"
HANDSHAKE_MAX_SKEW_S = 90
HANDSHAKE_FAILS_LOCKOUT = 10          # limit na pare (node, IP)
HANDSHAKE_FAILS_LOCKOUT_IP = 100      # zapora dla samego IP (anty-flood)
HANDSHAKE_LOCKOUT_S = 300

# Powody odrzucenia wysylane do klienta. Celowo zgrubne: endpoint jest
# nieuwierzytelniony, wiec nie potwierdzamy istnienia konkretnego node'a.
REJECT_AUTH = "auth"
REJECT_LOCKED = "locked"
# Zegar komputera rozjechany o wiecej niz HANDSHAKE_MAX_SKEW_S. Zdradza tylko
# to, co i tak widac po czasie odpowiedzi, a oszczedza godzin szukania klucza.
REJECT_CLOCK = "clock"
# Ponizsze dwa wysylane dopiero po poprawnym MAC (klient zna klucz).
REJECT_CASCADE = "cascade"
REJECT_VERSION = "version"

# Trwalosc ostatniego `declare` (odpowiednik retained discovery w MQTT)
STORAGE_VERSION = 1
PERSIST_DELAY_S = 5

# Log i zgloszenia w Naprawach
LOG_THROTTLE_S = 60
UNKNOWN_NODE_ISSUE_AFTER = 3
ISSUE_UNKNOWN_NODE = "unknown_node"

UNBOUND_TITLE = "Deskmate (oczekuje na parowanie)"

# Kod parowania: klucz + adresy HA w jednym ciagu do wklejenia w Deskmate
PAIRING_CODE_PREFIX = "DMP1."

# Kierunki szyfrowania (prefiks nonce)
DIR_C2S = b"\x01\x00\x00\x00"
DIR_S2C = b"\x02\x00\x00\x00"

SIGNAL_STATE = f"{DOMAIN}_state"          # + entry_id
SIGNAL_AVAILABLE = f"{DOMAIN}_available"  # + entry_id
SIGNAL_DECLARED = f"{DOMAIN}_declared"    # + entry_id

EVENT_NOTIFY_ACTION = f"{DOMAIN}_notify_action"
EVENT_TRIGGER = f"{DOMAIN}_trigger"
SIGNAL_TRIGGER = f"{DOMAIN}_trigger_sig"  # + entry_id + key

SERVICE_NOTIFY = "notify"

PLATFORMS = ["sensor", "binary_sensor", "switch", "number", "button", "text", "event"]

# Klucze redagowane w diagnostyce
REDACT = {CONF_KEY, CONF_CASCADE_KEY}
