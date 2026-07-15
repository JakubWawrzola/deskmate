# STATUS — Deskmate
Aktualizacja: 2026-07-15 (wydanie security hardening 0.3.1)

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
