# STATUS — Deskmate
Aktualizacja: 2026-07-19 (Deskmate Link, fala odbudowy HAOS 4)

## Sesja 2026-07-19 — Deskmate Link

- [x] Worktree wyrownany przez wymagane `git fetch` i
  `git reset --hard origin/main`, nastepnie branch `feature/deskmate-link`.
- [x] Rownolegly transport `mqtt|link`; stara konfiguracja i default pozostaja
  MQTT. Klucz Link jest tylko w Windows Credential Manager.
- [x] Hello/welcome HMAC, kontrola skew, HKDF, AES-256-GCM, rosnacy licznik,
  reconnect 2/3/5 s i rotacja local/remote zgodne z `DESKMATE-LINK.md`.
- [x] Link obsluguje `declare`, partial `state`, `cmd`/`ack`, `notify`,
  `notify_action`, `ping`/`pong` przez wspolne rejestry i handlery MQTT.
- [x] UI Settings i pierwszy kreator maja wybor transportu, URL, fallback i
  klucz. README oraz `docs/LINK.md` opisuja konfiguracje i ograniczenia.
- [x] Publiczny fixture zostal wygenerowany pythonowym `FrameCodec` z repo HA;
  test Rust potwierdza HMAC/HKDF, deszyfrowanie i blokade replayu.
- [x] Finalne `cargo check`, `cargo test` (2/2), `npx tsc --noEmit` i
  `cargo tree` przeszly. Drzewo nie zawiera ring/openssl/rustls.
- Commit: `5c3c706`. Bez builda, zywego HA, push i merge.
- Do manualnego E2E: parowanie, encje, komendy, toast/action, reconnect/fallback,
  kontrolowany replay i powrot do MQTT; pelna checklista jest w STATUS.md repo
  HomeAssistant, T20.

## Sesja 2026-07-15 — release 0.3.1 i installery

- [x] Wersja podniesiona z 0.3.0 do 0.3.1 we wszystkich metadanych aplikacji.
- [x] Dodano `docs/RELEASE-0.3.1.md` z gotowym opisem wydania na GitHub.
- [x] `docs/SECURITY.md` wskazuje zakres 0.3.1 i jawnie opisuje brak podpisu
  Authenticode/mozliwy komunikat SmartScreen.
- [x] `cargo check` i `npx tsc --noEmit` przeszly.
- [x] Zbudowano NSIS x64 i ARM64; oba maja ProductVersion 0.3.1.
- [x] `dist-installers/` zawiera tylko artefakty 0.3.1, SHA-256 i ZIP z obiema
  architekturami. ZIP zostal otwarty i ma oczekiwane trzy pliki.
- [x] Praca wykonana w `C:\dev\web\deskmate-codex` na `codex/work`.
- Brak push i merge. Installery sa niepodpisane; podpis wymaga certyfikatu.

## Sesja 2026-07-15 — security hardening po decyzji Kuby

- [x] MQTT TLS przez Windows Schannel (`rumqttc/use-native-tls`), domyslnie
  port 8883; opcjonalny PEM prywatnego CA. Plain MQTT jest swiadomym trybem
  `insecure` i nie pozwala ustawic fallback brokera.
- [x] Clipboard read i write rozdzielone na `Off / Confirm / Automatic`, oba
  default Off. Read confirm zatwierdza konkretna wartosc do jej zmiany; write
  confirm pyta za kazdym razem z preview. Oba blokuja sie przy lock screen;
  write ma 64 KiB i cooldown 2 s.
- [x] `open_url` i obrazy toast maja exact-origin allowliste; originy HA API sa
  auto-allowed, redirect obrazu blokowany.
- [x] Custom PowerShell commands maja osobne Enabled/Confirm, nowe i stare
  (migracja brakujacych pol) sa disabled + confirm required. Disabled encje sa
  usuwane z discovery.
- [x] Security audit log bez danych: `%APPDATA%\Deskmate\security.log`, rotacja
  1 MiB. Notification rate limit 10/min.
- [x] REST fallback jest HTTPS-only. README, SECURITY, HA-SETUP, ARCHITECTURE i
  ROADMAP zaktualizowane; SECURITY zawiera dokladny setup TLS + ACL HA Mosquitto.
- [x] Finalne `cargo check` i `npx tsc --noEmit` po ostatnich zmianach docs/kodu.
- Security hardening znalazl sie w lokalnym checkpointcie `bfe72b7`; bez push.

## Sesja 2026-07-15 — audyt bezpieczenstwa MQTT + REST

- [x] Dodano `docs/SECURITY.md`: model zagrozen, inwentaryzacja kanalow,
  priorytety problemow, stan poprawek i propozycje wymagajace decyzji Kuby.
- [x] Niskiego ryzyka hardening: odrzucanie MQTT retained na `cmd/+` i
  `notify`, rygorystyczne URL-e HTTP(S) bez credentials dla `open_url`, obrazow
  toast i HA REST, limity clipboard/TTS/toast, walidacja REST entity_id,
  nieserializowanie odpowiedzi HTTP do bledow.
- [x] W fazie pierwszej README opisalo stan 0.3.0 przed wdrozeniem decyzji.
  Aktualny stan po hardeningu opisuje sekcja wyzej i `docs/SECURITY.md`.
- [x] `cargo check` oraz `npx tsc --noEmit` przeszly.
- [x] Kuba zaakceptowal kolejna faze: TLS MQTT jako domyslne, ACL per node w
  dokumentacji, osobne tryby clipboard, allowlista URL-i, HTTPS-only dla
  fallback REST i rate limit notyfikacji zostaly wdrozone.
- Ten wpis opisuje historyczna faze audytu przed wdrozeniem decyzji.

## Cel biezacy
Odpowiedz na feedback z posta na r/homeassistant. Pelny opis w HANDOFF.md
sekcja "Sesja 2026-07-15 (po opublikowaniu na r/homeassistant)". Jakub
przechodzi teraz do pracy w Codex (limit tokenow) — HANDOFF.md ma pelny
kontekst dla kolejnego agenta.

## Zrobione (ta sesja) — WSZYSTKO UKONCZONE
1. Toast branding "HomeOS" NAPRAWIONY i POTWIERDZONY przez Kube (2 realne
   bledy C#/P-Invoke w notify.rs, szczegoly w HANDOFF.md).
2. Przyciski toastu DALEJ nie dzialaja — ODLOZONE na wyrazne polecenie Kuby,
   hipotezy spisane w HANDOFF.md i README.md ("Known issues").
3. Pelne tlumaczenie PL->EN: 14 plikow Rust, 8 plikow TS/TSX, 4 docsy
   planistyczne, resztki w streamdeck-plugin. cargo check + tsc (oba
   projekty) EXIT=0. HANDOFF.md/STATUS.md CELOWO zostaja po polsku.
4. README.md rozbudowany: spis tresci, akapit "what kind of app", akapit
   "why not HASS.Agent fork", sekcja "Known issues", sekcja "Screenshots"
   (tabela z placeholderami, CZEKA na pliki od Kuby).

## Nastepny krok (DOKLADNY)
Czeka na Kube:
- przetestowac lokalnie installery 0.3.1 wedlug checklisty ponizej
- po testach wrzucic artefakty z `dist-installers/` do GitHub Release 0.3.1;
  opis jest gotowy w `docs/RELEASE-0.3.1.md`
- dograe screenshoty do docs/screenshots/{status,sensors,hotkeys,widgets,
  notifications,settings}.png (instrukcja dana w czacie i w HANDOFF.md)
- powrot do buga z przyciskami toastu — WYLACZNIE na wyrazne polecenie

## Otwarte problemy / pulapki
- Przyciski toastu nie dzialaja - patrz HANDOFF.md, ODLOZONE.
- In-process WinRT toast.show() zawodzi u Kuby -> zawsze fallback
  PowerShell (to jest OK, oczekiwane, dziala).
- Zero ring/openssl/clang w deps (ARM64!). ureq+native-tls=schannel OK.
- RPi/HA byl niedostepny w trakcie tej sesji (aktualizacja HAOS) - zero
  zmian po stronie HA.
- Dev build appki mogl zostac uruchomiony w tle (target\debug\deskmate.exe)
  do testow live z Kuba — sprawdz `tasklist` na starcie kolejnej sesji.
- Branding shortcut HomeOS.lnk juz istnieje w Start Menu Kuby (utworzony
  podczas testu) - jesli trzeba wymusic ponowne utworzenie, usunac
  `%AppData%\Microsoft\Windows\Start Menu\Programs\HomeOS.lnk`.

## Kanoniczne fakty
- Worktree Claude: `C:\dev\web\deskmate` (`master`). Worktree Codex:
  `C:\dev\web\deskmate-codex` (`codex/work`). Protokol: `AGENTS.md`.
- Archiwum 0.2.3: `C:\dev\web\deskmate-0.2.3-archiwum` - NIE RUSZAC.
- Wersja: 0.3.1 (security hardening po feedbacku z Reddita).
- Build: npx tauri build --target x86_64-pc-windows-msvc / aarch64-pc-windows-msvc
- node_id laptopa Kuby: laptopwawrzola; broker LAN 192.168.18.9, TS 100.84.40.85
- HA: http://192.168.18.9:8123 (LAN), http://100.84.40.85:8123 (Tailscale)

## DO PRZETESTOWANIA / DO ZROBIENIA (zalegle u Kuby)
- Instalacja/upgrade 0.3.1 osobno na Windows x64 i ARM64.
- Migracja starego configu: TLS/8883, clipboard Off, custom commands disabled.
- Clipboard Confirm/Automatic w obu kierunkach oraz blokada przy lock screen.
- MQTT TLS z poprawnym i niepoprawnym certyfikatem oraz ACL per node.
- Allowlista `open_url`/obrazow i lokalne potwierdzenie custom PowerShell.
- Dograe 6 screenshotow do docs/screenshots/ (patrz wyzej)
- Przyciski toastu - ODLOZONE, nie ruszac bez polecenia
- Stream Deck plugin (brak sprzetu przy Kubie)
- LilyGo kalibracja dotyku (Kuba poza domem, wczesniejsza sesja)

## Fala 7 — T32 sensory sprzetowe

Status: wykonane offline; E2E na rzeczywistym sprzecie pozostaje manualne

- Dodano dynamiczne sensory GPU usage, VRAM used/total, wolnego miejsca i
  uzycia per wolumen, lacznego odczytu/zapisu dyskow oraz temperatur CPU/GPU.
- Zrodla sa lekkie i natywne: PDH (GPU), DXGI (calkowita pamiec GPU), WMI
  (Libre/OpenHardwareMonitor jako istniejacy provider, bez procesu-agenta,
  oraz fallback ACPI) i istniejace `sysinfo` dla wolumenow/transferow.
- Encje sprzetowe sa deklarowane przez MQTT discovery i Link `declare` dopiero
  po uzyskaniu prawidlowego odczytu. Znikniecie telemetrii usuwa retained
  discovery MQTT; ponowny Link `declare` umozliwia prune po stronie HA.
- Domyslnie sensory wykrytego sprzetu sa wlaczone i pojawiaja sie w istniejacej
  stronie Sensors. Niedostepny odczyt nie dostaje wartosci zastepczej ani encji.
- `cargo check`, `cargo test` (6/6), `npx tsc --noEmit` zakonczyly sie kodem 0.
  `cargo tree` ma 860 linii i nie zawiera ring/openssl/rustls.
- Nie uruchamiano aplikacji, MQTT, Link ani polaczenia z HA. Brak push i merge.

## Fala 7 — T34 Link Files v1

Status: wykonane offline; E2E przez integracje HA pozostaje manualne

- Dodano obsluge zaszyfrowanych ramek `fs`/`fs_res` dla operacji read-only
  `list`, `stat` i `read`. Protokol klienta nie ma operacji zapisu, zmiany
  nazwy ani kasowania.
- Nowe `link_file_roots` jest domyslnie pusta lista. Settings ma sekcje
  `File access (Link)` z jawnym ostrzezeniem, dodawaniem i usuwaniem rootow.
- Backend wymaga istniejacej absolutnej sciezki lokalnego dysku, canonicalize
  i zgodnosci komponentow z allowlista. Odrzuca `.`/`..`, UNC/device paths,
  ADS, symlinki i Windows reparse points; wpisy reparse sa pomijane w listingu.
- `read` ma limit 256 KiB/chunk, 16 MiB/file i globalny gate 4 MiB/s.
  Kazda operacja zapisuje op/path/wynik w rotowanym `security.log`, bez tresci.
- `cargo check`, `cargo test` (13/13), `npx tsc --noEmit` przeszly; finalna
  kontrola `cargo tree` nie zawiera ring/openssl/rustls.
- Nie wykonywano dostepu do prawdziwych plikow przez Link, polaczen z HA,
  uruchomienia aplikacji, push ani merge.

## Fala 7 — T35 finalizacja

Status: kod klienta wykonany offline; E2E pozostaje manualne

- Commity tej fali w Deskmate: `3d709d9` (dynamiczne sensory sprzetowe) i
  `e4c907d` (read-only Link Files v1). Finalny backup HomeAssistant przeszedl
  selftest: 2396 plikow, 20 008 960 B, skan sekretow i hashe bez bledow.
- Sensory sa gotowe dla MQTT discovery i Link declare, lecz karty nowych
  encji na dashboardzie Komputery wymagaja zmiany w strefie Claude'a.
- Klient Files jest gotowy, ale E2E z HA/Jarvis wymaga serwerowej czesci T36.
  Do tego czasu nalezy testowac lokalne odmowy walidacji i security log.
- Nie uruchamiano aplikacji ani rzeczywistego sprzetu, nie budowano
  instalatora, nie wykonywano deployu, merge ani push.

### DO PRZETESTOWANIA - Deskmate fala 7

1. Na Windows x64 i ARM64 potwierdzic wykrywanie GPU/VRAM, dyskow, transferow
   i dostepnych temperatur przez MQTT, a nastepnie Link; niedostepna metryka
   nie moze utworzyc encji.
2. Przy pustej allowliscie potwierdzic odmowe Files i wpis w `security.log`.
   Po wdrozeniu T36 dodac katalog testowy i sprawdzic list/stat/read.
3. Potwierdzic odmowe dla `..`, UNC/device, ADS, symlink/reparse point,
   wyjscia poza root i pliku ponad 16 MiB; log nie moze zawierac tresci.
