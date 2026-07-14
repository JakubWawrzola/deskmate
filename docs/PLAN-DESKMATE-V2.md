# PLAN WDROŻENIA — Deskmate v0.2

Zrodlo: workflow badawczy 2026-07-10 (4 agenty: inwentarz HASS.Agent, inne
companiony HAOS, audyt Deskmate, wykonalnosc Rust/Tauri ARM64 + synteza).
Baza: v0.1.2, 18 sensorow + 9 komend + custom PS, toast z obrazem, media SMTC,
czysty MQTT-discovery, rumqttc bez TLS, windows crate 0.61, ARM64 bez clanga.
Regula nadrzedna: zero ring/openssl-sys/C-buildu.

## STATUS (2026-07-10): WDROZONE
Kroki 2-6 + hardening zaimplementowane (cargo check + tsc zielone, installery
x64+ARM64 w dist-installers/). Krok 1 (branded toast) ODLOZONY do backlogu -
COM AUMID nie skompilowal sie na windows 0.61; toast dziala przez PowerShell
AUMID. Szczegoly: HANDOFF.md sekcja 0.2.0, testy: HomeAssistant/docs/
DO-PRZETESTOWANIA.md sekcja E.

## Zakres v0.2 (6 funkcji + hardening) — implementacja tej iteracji

Kryterium: zielone na ARM64, pasuje do czystego discovery (albo domyka bug),
najlepszy stosunek wartosc/naklad. Kolejnosc = ship order.

### Krok 1 — Branded toast (AUMID w skrocie Menu Start) — domyka bug 0.1.2
Skrot `%AppData%\Microsoft\Windows\Start Menu\Programs\Deskmate.lnk` z
`System.AppUserModel.ID` (IShellLinkW + IPropertyStore, PKEY
{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3},5, IPersistFile::Save). Potem powrot
do wlasnego TOAST_AUMID zamiast POWERSHELL_APP_ID. Skrot tworzony raz.
Pliki: notify.rs (ensure_start_menu_shortcut + przelaczenie AUMID), consts.rs.
Ryzyko: PROPVARIANT VT_LPWSTR (init/free), relogin zeby AUMID zaskoczyl.

### Krok 2 — Actionable notifications (przyciski toast -> MQTT) — wartosc 5
Payload notify o `actions:[{title,action}]`. tauri-winrt-notification 0.7->0.8:
add_button + on_activated -> publikacja na deskmate/<node>/notify/action.
Callback = Fn trzymajacy tokio Handle/mpsc (nie async closure).
Pliki: Cargo.toml (0.8), notify.rs, mqtt.rs, consts.rs, src/pages/Notifications.
Ryzyko: on_activated dziala tylko in-process gdy app zyje (trwaly tray = OK).

### Krok 3 — Schowek PC <-> HA (S) — wartosc 4, najtansze
arboard 3 (clipboard-win + windows-sys, zero C). Sensor
deskmate/<node>/clipboard (privacy=true, default OFF) + encja text
clipboard/set -> arboard set_text. Pliki: nowy clipboard.rs, sensors.rs,
mqtt.rs, discovery.rs (komponent text), Cargo.toml.
Ryzyko: najwrazliwszy sensor (hasla/2FA) - twarde opt-in + ostrzezenie w UI.

### Krok 4 — Zdalny wpis tekstu + sterowanie prezentacja (S) — wartosc 4
SendInput (Win32_UI_Input_KeyboardAndMouse - juz jest). Tekst KEYEVENTF_UNICODE.
Prezentacja: VK_RIGHT/LEFT/F5/ESCAPE/B. Encje: text type/set + buttony present_*.
Opt-in. Pliki: sys_commands.rs, mqtt.rs, discovery.rs, config.rs.
Ryzyko: UIPI (nie wysle do okien admin gdy app bez admina), whitelist zamiast
surowego passthrough.

### Krok 5 — TTS (PC mowi tekst z HA) (S/M) — wartosc 4
SAPI ISpVoice::Speak (Win32_Media_Speech), CLSID SpVoice, SPF_ASYNC. Kanal:
encja text tts/set (opt-in, flaga tts_enabled). Pliki: nowy tts.rs, mqtt.rs,
discovery.rs, config.rs, Cargo.toml, SettingsPage.
Ryzyko: SAPI = STA COM - dedykowany watek CoInitializeEx(APARTMENTTHREADED),
nie z tokio worker-poola; kolejkowanie tekstow.

### Krok 6 — switch / number w custom controls (S)
Custom kontrolki z configu: kind=button|switch|number, odpowiedni komponent
discovery, run_custom z typowana/walidowana wartoscia (jak volume, anty-RCE).
Pliki: config.rs (CustomControl.kind), sys_commands.rs, discovery.rs,
Commands.tsx, types.ts. Ryzyko: stan switch retained.

### Hardening (tanie fixy z audytu, wciagnac do v0.2)
- pominac pierwszy tick net_down/net_up (spike od startu), sensors.rs
- expire_after tez dla binary_sensor (session_locked/ac_power), discovery.rs
- czyszczenie starego wpisu Credential Manager przy zmianie host/user, config.rs
- czas historii notyfikacji lokalny zamiast UTC, mqtt.rs

## Backlog (nie teraz — z uzasadnieniem)
- Custom sensory usera (script->JSON), wartosc 5 = v0.3 opener (zmiana platformy,
  nie punktowa funkcja; buduje na switch/number + custom PS).
- Pelna encja media_player + okladka = v0.4 (HA nie ma MQTT-discovery
  media_player; wymaga HACS - lamie "zero HACS"). MVP zostaje przy SMTC.
- Screenshot on-demand = v0.4 (xcap dokłada 2. wersje windows crate; GDI = duzo
  kodu; inwazyjne prywatnosciowo).
- Transfer plikow = v0.3 (HTTP nie discovery; token w keyring, path traversal).
- Global hotkeys / quick-actions overlay = v0.3-0.5 (transparent window kapryśny,
  odwrotny kierunek PC->HA).
- TLS broker (mqtts) = czeka (rustls/ring wymaga clanga na ARM64).
- Satellite/usluga bez logowania = swiadomy brak (sprzeczne z modelem Tauri).
- Kamera/mik w uzyciu, Windows Updates, Process/Service State, per-core CPU,
  siec per-adapter, zewn IP, WebView, Wake-on-LAN = tanie, dokladac po v0.2.

## Checklisty DO PRZETESTOWANIA (Jakub) — dodawane do repo HomeAssistant/docs/DO-PRZETESTOWANIA.md sekcja E
Patrz per-krok wyzej; testy manualne, nic nie uruchamiam automatycznie.
