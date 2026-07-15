# HANDOFF — stan pracy i jak kontynuowac (dla KAZDEGO agenta: Claude/Codex/Antigravity)

> CZYTAJ TEN PLIK NAJPIERW. Potem `docs/PLAN.md` (taski + statusy [x]/[ ]).
> Nie czytaj calego codebase — ponizej jest mapa.

## Co to jest

**Deskmate** — open-source zamiennik HASS.Agent: aplikacja Windows 11 (x64 + ARM64)
laczaca komputer z Home Assistant przez MQTT discovery. Sensory systemowe, komendy
zdalne, powiadomienia toast z obrazem, media SMTC. Tauri 2 + React + TS + Tailwind.
Wlasciciel: Jakub Wawrzola. Cel: publikacja na GitHub (MIT), zero hardcode.

## Stan na 2026-07-10 ~02:00 (sesja nocna Claude)

- [x] T01-T12 NAPISANE I PRZECHODZA `cargo check` + `npx tsc --noEmit` (zielone).
  Caly core: config+keyring, MQTT+LWT+reconnect, discovery, 17 sensorow,
  9 komend built-in + custom PS, toast z obrazem, media SMTC, UI 5 stron +
  wizard, tray + autostart, dokumentacja (README/ARCHITECTURE/HA-SETUP/
  STREAMDECK-PLAN/ROADMAP/LICENSE).
- [x] T13 build release: OBA installery zbudowane (identifier
  com.deskmate.desktop, commit c6e844e):
  - ARM64 (zenbook): `src-tauri/target/release/bundle/nsis/Deskmate_0.1.0_arm64-setup.exe` (2.2 MB)
  - x64 (Ryzen):     `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Deskmate_0.1.0_x64-setup.exe` (2.5 MB)
- NIEPRZETESTOWANE RECZNIE (nikt nie odpalil appki!): pierwszy test wg
  DO PRZETESTOWANIA w PLAN.md robi Jakub. Najbardziej ryzykowne miejsca:
  touch WinRT toast w dev (AUMID), Activate() IAudioEndpointVolume,
  reconnect rumqttc, discovery format (literowki w JSON).
- Znane kompromisy: MQTT bez TLS (ring wymaga clanga na ARM64 — ROADMAP),
  czas w historii powiadomien w UTC (uproszczenie).

## 0.1.1 (2026-07-10) — fix powiadomien

- PROBLEM: toasty nie pokazywaly sie (ani "Send test toast", ani z HA), mimo
  ze MQTT dochodzil (node_id=laptopwawrzola potwierdzony w HA, topic OK).
- PRZYCZYNA: niepakietowana aplikacja Win wymaga AUMID zarejestrowanego w
  HKCU\Software\Classes\AppUserModelId\<aumid>; instalator NSIS tego NIE robi,
  wiec WinRT po cichu odrzucal toast.
- FIX 0.1.1 (NIEWYSTARCZAJACY): rejestracja AUMID w HKCU. Okazalo sie, ze
  wlasny AUMID renderuje toast TYLKO gdy jest skrot z AppUserModelID w Menu
  Start (MSIX albo recznie utworzony skrot) - sam wpis w HKCU nie wystarcza.
- FIX 0.1.2 (DZIALA): show_toast uzywa Toast::POWERSHELL_APP_ID - AUMID
  zarejestrowany w systemie ZAWSZE, wiec toast zawsze sie renderuje. Koszt:
  etykieta "Windows PowerShell" w rogu zamiast "Deskmate". ensure_aumid_
  registered() zostaje (nieszkodliwe). BRANDOWANIE = ROADMAP (skrot z AUMID
  w instalatorze NSIS przez nsis hook, albo pakiet MSIX).
- Wersja 0.1.2. Installery: dist-installers/ (+ zip). Trzeba PONOWNIE zainstalowac.

## 0.2.0 (2026-07-10) — funkcje z HASS.Agent (plan: docs/PLAN-DESKMATE-V2.md)

Research workflow (4 agenty + synteza) -> plan v0.2. Zaimplementowane, cargo check
+ tsc zielone. Regula utrzymana: zero ring/clang (ARM64 OK).
- **Actionable notifications**: payload notify o `actions:[{title,action}]`; klik
  przycisku -> publikacja `{action}` na `deskmate/<node>/notify/action` (drain w
  lib.rs czyta action_rx z AppState). winrt-notification 0.7->0.8. notify.rs+mqtt.rs.
- **Schowek PC<->HA**: sensor `clipboard` (privacy, OFF) + encja text `clipboard_set`
  (cmd/clipboard_set). arboard (clipboard-win, zero C). clipboard.rs. Gate = sensor opt-in.
- **Zdalny wpis tekstu + prezentacja**: encja text `type_text` (SendInput UNICODE) +
  buttony present_next/prev/start/black/end. Gate = config.allow_input. sys_commands.rs.
- **TTS**: encja text `tts_say` -> SAPI ISpVoice na wlasnym watku STA (tts.rs,
  kanal state.tts_tx). Gate = config.tts_enabled.
- **Custom switch/number**: CustomCommand.kind=button|switch|number; wartosc do PS
  jako $env:DESKMATE_VALUE (walidowana, anty-RCE). discovery.rs + config.rs.
- **Hardening**: net first-tick=0 (spike), expire_after tez binary_sensor,
  keyring cleanup przy zmianie host/user, czas historii toastow lokalny (GetLocalTime).
- **Branded toast (Krok 1) = ODLOZONE do backlogu**: skrot AUMID przez COM
  (IShellLink+IPropertyStore) - windows 0.61 nie eksponuje InitPropVariantFromString/
  PROPERTYKEY tak jak zakladano; toast dziala przez PowerShell AUMID (BRANDED=false).
  Do zrobienia przez hook NSIS (plugin ApplicationID). Patrz ROADMAP.
- Nowe opt-iny w Settings: "Remote input" (allow_input) + "Text-to-speech".
- Wersja 0.2.0. Installery + zip w dist-installers/.

## 0.2.1 (2026-07-13) — fix "Send test toast" + diagnostyka

- PROBLEM (Jakub): przycisk "Send test toast" nie robil nic - zaden toast.
- PRZYCZYNA: `void api.testToast()` POLYKAL wynik; gdy in-process WinRT `.show()`
  zwracal Err (zawodny apartment COM/WinRT w niepakietowanym procesie Tauri),
  uzytkownik nie widzial ani toastu, ani bledu.
- FIX (notify.rs): gdy in-process `.show()` zawiedzie -> fallback przez SWIEZY
  proces `powershell.exe` (CreateToastNotifier na POWERSHELL_APP_ID, CREATE_NO_WINDOW,
  XML escaped). Czyste srodowisko WinRT renderuje toast bez problemu z apartmentem.
  Fallback jest wizualny (bez callbacku przyciskow akcji - te dzialaja gdy in-process OK).
  Funkcje: `xml_escape`, `show_toast_powershell`.
- FIX (NotificationsPage.tsx): przycisk pokazuje wynik - "Toast sent..." (z podpowiedzia
  o Focus/Notifications gdy mimo to nic nie widac) albo CZERWONY komunikat bledu z backendu.
- Uwaga sieciowa (nie kod): "Network timeout" spoza domu = broker w LAN nieosiagalny.
  Rozwiazanie = Tailscale (host 100.84.40.85:1883). Opis: HomeAssistant/docs/DO-PRZETESTOWANIA.md sekcja B0.
- Wersja 0.2.1. Installery + zip w dist-installers/ (0.2.0 usuniete). cargo check + tsc zielone.

## 0.2.2 (2026-07-14) — feedback Jakuba (5 rzeczy)

Zgloszenia po tescie 0.2.1 + fixy:
- **Failover local/remote** (config.broker_host_remote + mqtt.rs): lista hostow
  [local, remote?]; klient probuje lokalny, po 2 nieudanych probach przelacza na
  zapasowy (Tailscale/public), po udanym zostaje. Puste pole remote = jeden host jak
  dawniej. UI: Settings pole "Fallback address (outside home)". Status pokazuje
  "Connecting (local/remote)...".
- **Czarne okno cmd** (sensors.rs wifi_ssid + sys_commands run_custom): dodane
  CREATE_NO_WINDOW. Migalo bo `netsh wlan show interfaces` (sensor WiFi SSID) leci co
  interwal bez flagi. run_custom PS tez dostalo flage.
- **Encje TTS/type_text natychmiastowe** (nowa komenda set_feature_flag + SettingsPage):
  przelaczniki allow_input/tts_enabled/toast_branding stosuja sie OD RAZU (zapis +
  republikacja discovery, jak set_sensor_enabled) — wczesniej wymagaly Save (stad
  "nie widze encji"). toast_branding wola apply_branding w tle.
- **Przyciski w test toascie** (lib.rs test_toast): dodane 2 przykladowe actions +
  action_tx (klik -> publikacja na notify/action). Uzytkownik moze zobaczyc/kliknac
  przyciski bez wysylania z HA. Jesli toast bez przyciskow = poszedl fallbackiem PS.
- **Branding "HomeOS"** (notify.rs apply_branding + ensure_start_menu_shortcut):
  tworzy skrot %AppData%\...\Start Menu\Programs\HomeOS.lnk z System.AppUserModel.ID
  = TOAST_AUMID przez PowerShell z wbudowanym C# (IShellLink+IPropertyStore;
  InitPropVariantFromString dziala w .NET, czego windows 0.61 nie dawal). Po sukcesie
  BRANDED=true -> toast pod wlasnym AUMID z DisplayName "HomeOS". Skrot tworzony raz
  (jesli istnieje -> od razu OK). Flaga config.toast_branding (default true) + toggle
  w Settings -> Notifications (bezpieczny wentyl: off = PowerShell AUMID, zawsze widoczny).
  Fallback PowerShell z 0.2.1 nadal chroni przypadek Err in-process.
- Wersja 0.2.2. cargo check + tsc zielone. Installery + zip w dist-installers/ (0.2.1 usuniete).
- NIEPRZETESTOWANE RECZNIE. Ryzyka: branded toast Ok-ale-niewidoczny (skrot nie zindeksowany)
  -> wentyl w Settings; failover ping-pong przy niestabilnej sieci (backoff 2-3s lagodzi).

## 0.2.3 (2026-07-14) — toast u Jakuba szedl fallbackiem PS (brak przyciskow + "Windows PowerShell")

Diagnoza ze zrzutu: in-process WinRT u Jakuba ZAWODZI, wiec KAZDY toast leci
fallbackiem PowerShell z 0.2.1 - a ten (a) hardcodowal POWERSHELL_APP_ID (etykieta
"Windows PowerShell"), (b) nie renderowal przyciskow. Fix = uczynienie sciezki PS
pelnoprawna:
- **Przyciski przez protokol** (notify.rs + lib.rs + consts.rs + Cargo): toast PS
  buduje `<actions>` z `activationType="protocol"` arguments=`deskmate:action?name=<a>`.
  Klik -> Windows uruchamia `deskmate:...` -> tauri-plugin-single-instance przekazuje
  URL do dzialajacej apki -> parse_action_url -> action_tx -> publikacja na notify/action.
  Nowe: notify::register_protocol() (HKCU Software\Classes\deskmate), pct_encode/
  pct_decode, parse_action_url. Single-instance = PIERWSZY plugin w builderze.
- **Branding w fallbacku** (notify.rs show_toast_powershell): AUMID = TOAST_AUMID gdy
  BRANDED (toast "HomeOS"), inaczej POWERSHELL_APP_ID. Wczesniej zawsze PowerShell.
- setup(): register_protocol() + obsluga wlasnego argv (gdy apka odpalona wprost z URL).
- Wersja 0.2.3. cargo check + tsc zielone. Installery + zip w dist-installers/ (0.2.2 usuniete).
- NIEPRZETESTOWANE. Ryzyka: pierwszy klik przycisku = Windows pyta o skojarzenie protokolu;
  gdyby przyciskow dalej nie bylo -> problem lezy w renderowaniu toastu, nie w akcjach.

## Stream Deck plugin (2026-07-14, agent fable) — `streamdeck-plugin/`

Osobny, samodzielny plugin Elgato Stream Deck (SDK v2, Node 20, TS, @elgato/streamdeck)
sterujacy HA BEZPOSREDNIO (REST akcje + WS stany na zywo). NIE zalezy od Deskmate.
3 akcje `com.homeos.homeassistant.*`: toggle-entity, call-service, activate-scene.
Klient HA singleton (auth WS, subscribe state_changed, reconnect backoff). Config
(HA URL + long-lived token) w Property Inspectorze (global settings), zero hardcode.
`npm install` + `npm run build` (rollup) PRZESZLY, tsc zielone. Instrukcja + testy
reczne: `streamdeck-plugin/README.md`. Plan/stan: `docs/STREAMDECK-PLAN.md`.
NIEPRZETESTOWANE na sprzecie (Jakub nie ma Stream Decka przy sobie).

## 0.3.0 (2026-07-15) — "Control Anywhere" (plan: docs/PLAN-DESKMATE-V3.md)

Duzy pakiet: sterowanie HA z komputera (dotad tylko HA -> PC). Kopia archiwalna
poprzedniej wersji: C:\dev\web\deskmate-0.2.3-archiwum (NIE RUSZAC).
- **HA API (F1)**: ha_api.rs - REST (call_service/get_state/ping), URL lokalny +
  fallback (failover na blad transportu, NIE na 4xx), token w Credential Manager
  ("Deskmate HA Token"). Settings -> "Home Assistant API" + Save & test.
- **Hotkeye (F2/F2a)**: hotkeys.rs + tauri-plugin-global-shortcut; akcje typu
  ActionSpec (toggle/service/command/widget/mqtt) przez actions.rs; kind=mqtt
  publikuje deskmate/<node>/hotkey/<id> + discovery device_automation (trigger
  w edytorze automatyzacji HA). Strona Hotkeys w UI. Normalizacja acceleratorow
  (L->KeyL, 2->Digit2) w hotkeys::normalize.
- **Widgety (F3)**: okno "widget" (tauri.conf, hidden, alwaysOnTop, frameless,
  index.html#widget -> WidgetPanel.tsx), kafelki encji, polling widget_states
  co 3 s + optymistyczny toggle. Konfiguracja: strona Widgets.
- **Tray quick actions (F4)**: config.tray_actions + rebuild_tray_menu (qa_<id>),
  edycja na stronie Hotkeys. Tray ma tez "Show/hide widgets".
- **Nowe encje/komendy (F5-F8)**: switch keep_awake (dedykowany watek
  SetThreadExecutionState - spawn_keep_awake, kanal w AppState), binary sensory
  camera_in_use/mic_in_use (ConsentStore, privacy opt-in), button
  empty_recycle_bin (SHEmptyRecycleBinW), text open_url (TYLKO http/https,
  gate allow_input).
- **Docs EN (F9)**: README (pelny feature list), ROADMAP, ARCHITECTURE (+sekcja
  v0.3), HA-SETUP (+sekcje 7/8), streamdeck-plugin/README -> wszystko EN.
  Robocze (HANDOFF/PLAN-*/STATUS) zostaja PL.
- **Dashboard HAOS (F10)**: HomeAssistant/dashboards/komputery.yaml (sekcje:
  Laptop=laptopwawrzola mushroom+mini-graph, Sterowanie, PC=zaslepka onboarding)
  + wpis lovelace-komputery w configuration.yaml. WDROZONE na RPi przez SMB
  Tailscale + restart Core webhookiem (2026-07-15 ~00:52, HA wstalo, HTTP 200).
- Wersja 0.3.0. cargo check + tsc zielone, installery x64+ARM64 + zip w
  dist-installers/ (0.2.3 usuniete z dist).
- NIEPRZETESTOWANE RECZNIE. Ryzyka: nazwy encji w komputery.yaml zgadywane ze
  slugow (Jakub zweryfikuje w HA), globalne skroty moga kolidowac z innymi
  aplikacjami (blad zwracany per-hotkey), widget okno na multi-monitor.

## Sesja 2026-07-15 (po opublikowaniu na r/homeassistant) — feedback, fix brandingu, pelne tlumaczenie EN

Jakub opublikowal repo (0.3.0, jeszcze niezacommitowane wtedy) na Facebooku i na
r/homeassistant. Ten wpis to pelny kontekst dla kolejnego agenta (Codex/inny),
zeby nie trzeba bylo czytac calej historii sesji.

### Feedback z Reddita (PDF od Jakuba, r/homeassistant, post "DeskMate - HASS.agent
### modern alternative", user Vast-Pipe-9362 = Jakub)

Post mial ~12k views, glowne komentarze (chronologicznie):

1. **DarkAutumn (bezpieczenstwo)** — najbardziej tresciwy komentarz:
   - MQTT jest nieszyfrowany -> skompromitowane urzadzenie IoT/router/malware w
     sieci moze wysylac komendy do deskmate bez autoryzacji poza samym dostepem
     do brokera.
   - `open_url` bez limitu docelowego adresu = building block dla SSRF (nie
     exploit sam w sobie, ale przydatny atakujacemu do probingu sieci).
   - Dostep do schowka (odczyt+zapis) = mozna odpytywac "co jest w schowku"
     szukajac hasel/tokenow, albo podmienic zawartosc licząc ze user wklei
     zlosliwy payload gdzies indziej.
   - Uruchamianie skonfigurowanych skryptow PowerShell — "zalezy co jest
     skonfigurowane".
   - Konkluzja: "wouldn't install this on my machine". Jakub odpowiedzial:
     zgadzam sie, zajme sie tym w kolejnej iteracji, nie przez AI tylko recznie.
   - **Jakub explicite powiedzial w TEJ sesji: bezpieczenstwem zajmie sie POZNIEJ,
     osobno. NIE dotykac tego tematu bez wyraznego polecenia.**
2. **groogs** — feedback UX/dokumentacji, nie security:
   - Brak screenshotow = trudno ocenic czy warto zaglebiac sie w projekt.
   - Niejasne czy to appka desktopowa, czy dziala w tle/tray, czy ma serwis.
   - Polowa plikow/kodu po polsku = bariera dla wspolpracy.
   - Jakub odpowiedzial: tak, to appka desktopowa, dziala w tray; przetlumacze
     wszystko w nastepnej iteracji; dodal screeny komend na biezaco w komentarzu.
3. **aevans0001 / Dazman_123** — pytania czy Jakub faktycznie zweryfikowal
   bezpieczenstwo kodu pisanego z AI, czy tylko "poprosil AI zeby bylo bezpieczne".
   Jakub: konsultowal sie z prowadzacym (wykladowca, tez zna HA), kod
   wrazliwych miejsc pisal recznie sam, feedback wezmie pod uwage w kolejnej
   wersji.
4. **pgsz / svkowalski** — informacyjnie: HASS.Agent zostal zforkowany i jest
   aktywnie rozwijany (`github.com/hass-agent/HASS.Agent`), fork rozwiazuje
   wiekszosc problemow oryginalu (ale nadal wymaga MQTT). Jakub: moj glowny
   problem z forkiem to brak niektorych sensorow/stanow i brak wsparcia ARM64.
5. FluffyRabbit — zart ("kazdy nowy projekt AI to 'modern alternative'"), bez
   akcji.

### Co zrobione w tej sesji (bez security — to na pozniej)

1. **FIX brandingu toastu "HomeOS"** (notify.rs) — 2 realne bledy, oba od
   0.2.2/0.2.3 (NIGDY wczesniej nie dzialalo, mimo ze kod "wygladal dobrze"):
   - C# `public static class Native` eksponowal w sygnaturze metody
     `internal struct PROPVARIANT` -> **blad kompilacji C#** "Inconsistent
     accessibility" -> `Add-Type` zawsze konczyl sie exit 1 -> `ensure_start_
     menu_shortcut()` zawsze Err -> BRANDED zawsze false -> toast ZAWSZE szedl
     jako "Windows PowerShell". FIX: `Native` -> `internal`.
   - `InitPropVariantFromString` z `propsys.dll` **nie istnieje jako eksport
     DLL** (to funkcja inline z naglowka `propvarutil.h`, kompilowana do kodu
     wywolujacego, nie eksportowana) -> P/Invoke zawsze rzucal
     `EntryPointNotFoundException`. FIX: reczne zbudowanie PROPVARIANT przez
     `SHStrDupW` (rzeczywisty eksport z `shlwapi.dll`) + `vt = 31` (VT_LPWSTR).
   - Diagnoza zrobiona live: uruchomiony dev build (`cargo run` + `npm run
     dev`), log z `RUST_LOG=debug` (Start-Process -Environment), skrypt
     PowerShell odtworzony i uruchomiony osobno zeby zobaczyc realny blad C#
     (wczesniej `ensure_start_menu_shortcut` polykala stderr PowerShella —
     TERAZ TEZ NAPRAWIONE: `.output()` zamiast `.status()`, stderr w komunikacie
     bledu, wiec przyszle awarie brandingu beda widoczne w logu).
   - **POTWIERDZONE PRZEZ JAKUBA na dev buildzie**: etykieta toastu to teraz
     "HomeOS", nie "Windows PowerShell". DZIALA.
2. **PRZYCISKI W TOAST NADAL NIE DZIALAJA** — mimo poprawnej etykiety i mimo
   ze `show_toast_powershell` buduje poprawny `<actions>` XML z
   `activationType="protocol"` (`deskmate:action?name=...`), przyciski sie NIE
   pokazuja pod toastem u Jakuba. **Jakub kazal to zostawic na razie — NIE
   grzebac dalej bez wyraznego polecenia.** Hipotezy niesprawdzone (do zrobienia
   pozniej, gdy Jakub da zielone swiatlo):
   - Windows moze wymagac COM background activatora (`ToastActivatorCLSID` w
     wlasciwosciach skrotu) do renderowania interaktywnych przyciskow, a samo
     `activationType="protocol"` moze nie wystarczac dla przyciskow (choc
     dziala dla klikniecia calego toastu wg dokumentacji MS — nie
     zweryfikowane w tym przypadku).
   - Focus Assist / ustawienia powiadomien per-aplikacja w Windows na maszynie
     Jakuba.
   - Swiezy proces `powershell.exe` (bez pelnej tozsamosci pakietu/AUMID
     powiazanego z realna, dzialajaca aplikacja) moze miec ograniczone prawa do
     renderowania przyciskow mimo poprawnego XML.
   - Do zrobienia: sprawdzic faktyczny wpis w Windows Action Center (prawy
     klik -> wlasciwosci powiadomienia) i/lub `Get-StartApps`/rejestr, zeby
     zobaczyc czy Windows w ogole "widzi" AUMID jako pelnoprawna aplikacje.
3. **PELNE TLUMACZENIE PL -> EN** (kod + docsy, do publikacji na GitHub):
   - Wszystkie pliki Rust w `src-tauri/src/`: `clipboard.rs`, `config.rs`,
     `discovery.rs`, `ha_api.rs`, `hotkeys.rs`, `lib.rs`, `mqtt.rs`, `notify.rs`,
     `sensors.rs`, `sys_commands.rs`, `tts.rs`, `consts.rs`, `state.rs`,
     `actions.rs` — komentarze/docstringi/komunikaty logow przetlumaczone,
     ZERO zmian w logice/nazwach/identyfikatorach protokolu (tematy MQTT,
     klucze JSON, ID komend, identyfikatory encji HA).
   - Frontend: `src/api.ts`, `src/components.tsx`, `src/pages/SettingsPage.tsx`,
     `src/pages/Wizard.tsx`, `src/pages/HotkeysPage.tsx`,
     `src/pages/WidgetPanel.tsx`, `src/pages/WidgetsPage.tsx`.
   - `streamdeck-plugin/`: resztki polskiego w `src/actions/activate-scene.ts`,
     `src/actions/call-service.ts`, `src/actions/toggle-entity.ts`,
     `src/ha-client.ts`, `src/plugin.ts` — wyczyszczone.
   - Docsy: `docs/PLAN-DESKMATE-V2.md`, `docs/PLAN.md`, `docs/PLAN-DESKMATE-V3.md`,
     `docs/STREAMDECK-PLAN.md` — pelne tlumaczenie. `README.md` / `ARCHITECTURE.md`
     / `HA-SETUP.md` / `ROADMAP.md` / `streamdeck-plugin/README.md` juz byly
     po angielsku z poprzedniej sesji (potwierdzone, bez zmian poza README).
   - **CELOWO ZOSTAWIONE PO POLSKU**: `HANDOFF.md` (ten plik) i `STATUS.md` —
     to robocze pliki do kontynuacji sesji miedzy agentami, nie user-facing
     dokumentacja. Do potwierdzenia z Jakubem czy to ma sie zmienic przed
     publikacja (jesli tak, przetlumaczyc i te dwa).
   - Weryfikacja: `cargo check --no-default-features` (deskmate) - EXIT 0;
     `npx tsc --noEmit` (deskmate i streamdeck-plugin) - EXIT 0, zero bledow.
     Wielokrotny sweep (Python, regex na polskie slowa + diacritics) na calym
     repo (poza HANDOFF/STATUS) - CZYSTO po fixach.
4. **README.md rozbudowany**:
   - Nowa sekcja "Screenshots" (tabela 2x3, placeholdery `docs/screenshots/
     {status,sensors,hotkeys,widgets,notifications,settings}.png` - Jakub ma
     je dograc recznie, patrz instrukcja ponizej w tym samym wpisie).
   - Spis tresci (Table of contents).
   - Akapit "What kind of app is this?" — odpowiada wprost na pytanie groogs
     z Reddita (tray app, brak osobnego serwisu, dziala w sesji usera).
   - Akapit "Why not just use HASS.Agent (or its active fork)?" — odpowiada
     na pytanie pgsz/svkowalski, uczciwie (nie deprecjonuje forka), powod =
     ARM64 + konkretne sensory ktorych brakowalo Jakubowi.
   - Nowa sekcja "Known issues" — transparentnie opisuje: (a) przyciski toastu
     czasem sie nie renderuja (patrz wyzej), (b) in-process WinRT bywa
     zawodne i fallback PowerShell to oczekiwane zachowanie, nie bug.
5. **Jak Jakub ma dodac screenshoty** (instrukcja dana w czacie): zrobic
   Win+Shift+S na oknie appki dla kazdej z 6 zakladek (Status, Sensors,
   Hotkeys, Widgets, Notifications, Settings), zapisac jako PNG w
   `docs/screenshots/` pod DOKLADNIE nazwami `status.png`, `sensors.png`,
   `hotkeys.png`, `widgets.png`, `notifications.png`, `settings.png` — README
   juz je referencuje wzglednymi sciezkami, wiec po prostu wrzucenie plikow o
   tych nazwach do folderu wystarczy, zero dalszej edycji README potrzebne.

### Stan repo NA KONIEC tej sesji

- Wersja: 0.3.0 (bez zmiany numeru w tej sesji — same fixy/tlumaczenie/docsy).
- **BEZ COMMITA nadal obowiazuje** — Jakub zakazal w POPRZEDNIEJ sesji ("nie
  rob commita na github") i to NIE zostalo cofniete. Working tree w
  `C:\dev\web\deskmate` ma sporo niezacommitowanych zmian (branding fix +
  cale tlumaczenie + README). Kolejny agent: NIE commituj bez wyraznego,
  swiezego polecenia Jakuba w biezacej rozmowie.
- Dev build appki byl uruchomiony (`target\debug\deskmate.exe`, dev + `npm run
  dev` na porcie 1420) do testow live brandingu z Jakuba — moze wciaz dzialac
  w tle, sprawdz `tasklist` i ewentualnie zamknij, jesli koliduje z czyms.
  Skrot `HomeOS.lnk` w Start Menu Jakuby juz istnieje (utworzony podczas testu)
  — jesli trzeba wymusic ponowne wygenerowanie, usunac
  `%AppData%\Microsoft\Windows\Start Menu\Programs\HomeOS.lnk` przed testem.
- RPi/HAOS Jakuba byl NIEDOSTEPNY w trakcie tej sesji (self-update) — zero
  zmian po stronie HA (dashboard komputery.yaml z poprzedniej sesji sie NIE
  zmienil, nie zostal ponownie zweryfikowany).
- Otwarte TODO nastepnej sesji: (a) Jakub dograa screenshoty, (b) ewentualny
  powrot do buga z przyciskami toastu na wyrazne polecenie, (c) bezpieczenstwo
  (MQTT bez TLS, `open_url`/schowek/PowerShell scripts) — WYLACZNIE gdy Jakub
  o to poprosi, (d) commit — WYLACZNIE na wyrazne polecenie.

## 0.3.1 (2026-07-15) — security hardening i nowe installery (Codex)

- Od checkpointu `bfe72b7` praca Codexa odbywa sie w dedykowanym worktree
  `C:\dev\web\deskmate-codex` na branchu `codex/work`. Pelny protokol jest w
  `AGENTS.md`; nie wolno samodzielnie merge'owac ani pushowac.
- Pakiet zabezpieczen po feedbacku DarkAutumn jest wdrozony: MQTT TLS/Schannel,
  osobne tryby clipboard read/write, exact-origin allowlista URL-i, lokalne
  potwierdzenia custom PowerShell, limity/replay protection/audit log oraz
  HTTPS-only dla fallback REST. Szczegoly i residual risks: `docs/SECURITY.md`.
- Wersja podniesiona do 0.3.1. Gotowy opis GitHub Release jest w
  `docs/RELEASE-0.3.1.md`.
- `cargo check` i `npx tsc --noEmit` przeszly. Zbudowano installery NSIS x64 i
  ARM64, sprawdzono ProductVersion 0.3.1 oraz zawartosc ZIP.
- Artefakty w `dist-installers/`: `Deskmate_0.3.1_x64-setup.exe`,
  `Deskmate_0.3.1_arm64-setup.exe`, `Deskmate_0.3.1_installers.zip` i
  `SHA256SUMS.txt`. Installery nie maja podpisu Authenticode; SmartScreen moze
  pokazac unknown publisher. Nie podpisywac bez certyfikatu Jakuba.
- SHA-256 ZIP: `C685AAF21A7A1FBC5DA195E3A79B09E2E7097C217F336B7716F0A71D09134036`.
- Jakub wyrazil swieza zgode na lokalny commit na `codex/work`; nadal brak
  zgody na push lub merge.

## Jak wznowic prace

1. Przeczytaj `AGENTS.md`, `AGENT-LOG.md`, ten plik i `docs/PLAN.md`.
2. Pracuj tylko w worktree przypisanym agentowi i sprawdz w nim
   `git log --oneline -10`.
3. Kontynuuj pierwszy task `[ ]` w ship order. Po KAZDYM tasku:
   - odhacz `[x]` w PLAN.md,
   - zaktualizuj sekcje "Stan na" w tym pliku (data + co zrobione),
   - commit (conventional commits, EN).
4. NIE uruchamiaj `tauri dev`/`tauri build` — tylko `npx tsc --noEmit` i `cargo check`.
   Testy manualne robi Jakub (sekcje DO PRZETESTOWANIA w PLAN.md).

## Mapa kodu (aktualizuj przy zmianach!)

```
deskmate/
├── HANDOFF.md            <- TEN PLIK
├── docs/
│   ├── PLAN.md           <- taski + statusy + testy manualne
│   ├── ARCHITECTURE.md   <- decyzje techniczne (T12)
│   ├── HA-SETUP.md       <- konfiguracja po stronie HA (T12)
│   └── STREAMDECK-PLAN.md<- plan + stan integracji Elgato (MVP zrobione 2026-07-14)
├── streamdeck-plugin/    <- SAMODZIELNY plugin Stream Deck (Node 20 + TS, REST/WS
│                            bezposrednio do HA, bez Deskmate). START: jego README.md
├── src/                  <- React UI (monochrom bialo-czarny)
│   ├── App.tsx           <- router stron + wizard gate
│   ├── pages/            <- Status, Sensors, Commands, Notifications, Settings, Wizard
│   └── styles.css        <- design tokens (Tailwind 4)
└── src-tauri/
    ├── tauri.conf.json
    └── src/
        ├── lib.rs        <- setup, AppState, tauri commands, tray
        ├── consts.rs     <- WSZYSTKIE stale nazwy (rename tu)
        ├── config.rs     <- %APPDATA%\Deskmate\config.json + keyring (haslo)
        ├── mqtt.rs       <- rumqttc client, reconnect, LWT, router wiadomosci
        ├── discovery.rs  <- HA MQTT discovery (device + encje, retained)
        ├── sensors.rs    <- sysinfo + WinAPI, petla publikacji, opt-in privacy
        ├── sys_commands.rs <- lock/shutdown/sleep/volume/custom PS (NIGDY eval payloadu!)
        ├── notify.rs     <- topic notify -> toast WinRT z obrazem
        └── media.rs      <- SMTC: sensory utworu + play/pause/next/prev
```

## Srodowisko dev (ten laptop: Zenbook ARM64)

- node 24, npm 11, rust 1.95 (toolchainy: aarch64 default + x86_64)
- Docelowe maszyny Jakuba: zenbook (ARM64) + Ryzen 7800X3D (x64), oba Win11
- HA Jakuba do testow: HAOS na RPi, LAN 192.168.18.9, Tailscale 100.84.40.85
  (broker MQTT = dodatek Mosquitto; NIE hardcodowac nigdzie w kodzie!)

## Twarde zasady

- ZERO hardcode (adresy, nazwy, sciezki uzytkownika) — wszystko z configu.
- Sensory privacy-sensitive (aktywne okno, media, SSID) DOMYSLNIE off, opt-in w UI.
- MQTT payload NIGDY nie jest wykonywany — komendy tylko po ID z configu.
- Haslo brokera tylko w Windows Credential Manager, nie w config.json.
- UI: monochrom bialo-czarny (wzorzec: dashboardy Dom/budzet Jakuba), bez emoji,
  bez gradientow. Teksty UI po angielsku.
- Kazdy commit: `npx tsc --noEmit` + `cargo check` zielone.
