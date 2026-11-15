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
HANDSHAKE_FAILS_LOCKOUT = 10  # limit na pare (node, IP)
HANDSHAKE_FAILS_LOCKOUT_IP = 100  # zapora dla samego IP (anty-flood)
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

UNBOUND_TITLE = "Deskmate (waiting for pairing)"
# Tytul wpisow sprzed 0.7.3; zamieniany przy starcie na UNBOUND_TITLE.
LEGACY_UNBOUND_TITLE = "Deskmate (oczekuje na parowanie)"

# Kod parowania: klucz + adresy HA w jednym ciagu do wklejenia w Deskmate
PAIRING_CODE_PREFIX = "DMP1."

# Kierunki szyfrowania (prefiks nonce)
DIR_C2S = b"\x01\x00\x00\x00"
DIR_S2C = b"\x02\x00\x00\x00"

# The `info` frame (docs/LINK.md, "Session frames"): what a client may say
# about itself, and the optional frame types this integration announces.
ROLE_SERVICE = "service"
INFO_ROLES = frozenset({"app", ROLE_SERVICE, "daemon"})
INFO_PLATFORMS = frozenset({"windows", "linux", "android_tv"})
INFO_APP_VERSION_PATTERN = r"^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]{1,20})?$"
LINK_CAPS = ("info", "scoped_declare", "bye", "media_player", "notify_tag")
# What a client may list in the optional `features` of its `info`. The hub
# sends what a feature adds only to a session that listed it, and ignores a
# name it does not know.
FEATURE_MEDIA_PLAYER = "media_player"
FEATURE_NOTIFY_TAG = "notify_tag"
CLIENT_FEATURES = frozenset({FEATURE_MEDIA_PLAYER, FEATURE_NOTIFY_TAG})
INFO_FEATURES_MAX = 16
INFO_FEATURE_PATTERN = r"^[a-z][a-z0-9_]{0,31}$"

# `declare` with the scope "machine" lists only what a client without a
# signed-in user can serve. Every other declaration is a full one.
DECLARE_SCOPE_MACHINE = "machine"
DECLARE_SCOPE_FULL = "full"

# The `bye` frame: why a client is about to close the socket.
BYE_HANDOVER = "handover"
BYE_SUSPEND = "suspend"
BYE_SHUTDOWN = "shutdown"
BYE_REASONS = frozenset({BYE_HANDOVER, BYE_SUSPEND, BYE_SHUTDOWN})
# After `bye` with the reason `handover`, the computer's other client is about
# to connect. The computer stays available for this long while it does.
HANDOVER_WINDOW_S = 15
# A session makes the computer available with its first `declare`. A client
# that sends none gets there this long after the handshake.
DECLARE_WAIT_S = 3

# States of the Connection sensor, which the hub adds to every computer. A
# session whose client sent `info` shows that client's role instead of
# "connected".
CONNECTION_CONNECTED = "connected"
CONNECTION_SLEEPING = "sleeping"
CONNECTION_OFF = "off"
CONNECTION_DISCONNECTED = "disconnected"
CONNECTION_STATES = (
    *sorted(INFO_ROLES),
    CONNECTION_CONNECTED,
    CONNECTION_SLEEPING,
    CONNECTION_OFF,
    CONNECTION_DISCONNECTED,
)

SIGNAL_STATE = f"{DOMAIN}_state"  # + entry_id
SIGNAL_AVAILABLE = f"{DOMAIN}_available"  # + entry_id
SIGNAL_DECLARED = f"{DOMAIN}_declared"  # + entry_id
SIGNAL_CONNECTION = f"{DOMAIN}_connection"  # + entry_id

EVENT_NOTIFY_ACTION = f"{DOMAIN}_notify_action"
EVENT_TRIGGER = f"{DOMAIN}_trigger"
SIGNAL_TRIGGER = f"{DOMAIN}_trigger_sig"  # + entry_id + key

SERVICE_NOTIFY = "notify"
SERVICE_CLEAR_NOTIFICATION = "clear_notification"
SERVICE_TV_UPDATE = "tv_update"

# DeskmateTV: katalog z APK do aktualizacji (/config/deskmate_tv) i
# rozmiar fragmentu (192 KB surowych = 256 KB base64 w jednej ramce)
TV_UPDATE_DIR = "deskmate_tv"
TV_UPDATE_CHUNK = 192 * 1024
SERVICE_SEND_FILE = "send_file"
SERVICE_FETCH_FILE = "fetch_file"

# Deskmate Files (strona w pasku bocznym + API HTTP)
FILES_PANEL_URL = "deskmate-files"
# Strona w sidebarze: pliki + lista komputerow. URL zostaje "deskmate-files",
# zeby nie gubic kolejnosci/ukrycia w sidebarze ustawionych przez uzytkownikow.
FILES_PANEL_TITLE = "Deskmate"
FILES_PANEL_ICON = "mdi:monitor-share"
FILES_STATIC_URL = "/deskmate_link_static"
FILES_LINK_CHUNK = 256 * 1024  # jedna ramka Link (limit po stronie komputera)
FILES_HTTP_CHUNK = 8 * 1024 * 1024  # jedno zadanie HTTP z przegladarki
FILES_UPLOAD_IDLE_S = 600

PLATFORMS = [
    "sensor",
    "binary_sensor",
    "switch",
    "number",
    "button",
    "text",
    "event",
    "select",
    "media_player",
]

# Wartosci regul rodzinnych na chronionym koncie: HA odklada je ponownie,
# gdy komputer zglosi cos innego - nie czesciej niz co tyle sekund.
OWNED_PUSH_EVERY_S = 30

# Klucze redagowane w diagnostyce
REDACT = {CONF_KEY, CONF_CASCADE_KEY}
