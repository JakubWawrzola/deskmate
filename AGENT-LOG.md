# Agent activity log

Protocol: see [AGENTS.md](AGENTS.md). Read the IN PROGRESS table below before
editing anything. Append your own entry after finishing a task — never edit
or delete another agent's entry.

## IN PROGRESS (update before you start, clear when you finish)

| Area | Agent | Since |
|---|---|---|

## Log

## [2026-07-15 ~15:30] Claude — multi-agent coordination setup
Zrobione: AGENTS.md, AGENT-LOG.md, worktree C:\dev\web\deskmate-codex (branch
codex/work) utworzony od checkpointu na master. CLAUDE.md w tym repo z
pointerem na ten protokol.
Dotkniete pliki: AGENTS.md, AGENT-LOG.md, CLAUDE.md (nowe)
Nastepny krok: Codex przechodzi na worktree deskmate-codex dla kolejnych
sesji; obecna sesja bezpieczenstwa dokonczona/zacommitowana na master jako
czesc checkpointu.

## [2026-07-15 ~11:50] Codex — security hardening release 0.3.1
Zrobione: zweryfikowano audit MQTT/REST z checkpointu bfe72b7, podniesiono
wersje do 0.3.1, doprecyzowano publiczne SECURITY i dodano gotowe release
notes. Cargo check i tsc przeszly. Zbudowano NSIS x64+ARM64, utworzono ZIP i
SHA256SUMS, sprawdzono wersje, hash kopii i zawartosc archiwum. Installery sa
niepodpisane (jawnie udokumentowane). Brak push i merge.
Dotkniete pliki: metadane package/Cargo/Tauri, README.md, docs/SECURITY.md,
docs/RELEASE-0.3.1.md, HANDOFF.md, STATUS.md, dist-installers/*, AGENT-LOG.md.
Nastepny krok: Jakub testuje upgrade/instalacje i funkcje security; po testach
moze opublikowac cztery artefakty z dist-installers jako GitHub Release 0.3.1.

## [2026-07-15 ~12:53] Codex — publikacja GitHub v0.3.1
Zrobione: codex/work scalony lokalnie do master; dolaczono niezalezna historie
starego zdalnego main bez force-pusha. Cargo check i tsc przeszly na wyniku
merge. Zdalny main zaktualizowano do security hardening 0.3.1 przez Git Data
API, bo lokalny Git for Windows zawieszal transport HTTPS przed polaczeniem.
Kod, docs, screenshoty i SHA256SUMS sa na main; trzy duze EXE/ZIP celowo sa
wylacznie assets release, bez dublowania ich w historii zdalnego repo.
Release: https://github.com/JakubWawrzola/deskmate/releases/tag/v0.3.1
Status: publiczny, Latest, nie draft/prerelease. Cztery assets porownane z
lokalnymi po rozmiarze i SHA-256. Opis jawnie zaznacza brak ponownego testu
end-to-end z HA, bo HA byl niedostepny. Tag v0.3.1 wskazuje commit 1be1607.
Nastepny krok: po powrocie HA Jakub wykonuje manualna checkliste z STATUS.md;
ewentualne problemy ida do kolejnego patch release, bez podmiany v0.3.1.

## [2026-07-19] Codex — przygotowanie worktree do Deskmate Link
Zrobione: zgodnie z fala 4 wykonano `git fetch origin`, a nastepnie jawnie
zlecony `git reset --hard origin/main`. Worktree zostal wyrownany do commita
`bac7c8b`, po czym utworzono branch `feature/deskmate-link` bez push i merge.
Dotkniete pliki: AGENT-LOG.md.
Nastepny krok: T17 i T19 w repo HomeAssistant, potem implementacja klienta
Deskmate Link na tym branchu.

## [2026-07-19] Codex — T18 klient Deskmate Link
Zrobione: dodano wybieralny transport `mqtt|link` (domyslnie MQTT), klienta
WebSocket z hello/welcome, HMAC, HKDF, AES-256-GCM, licznikami anti-replay,
rotacja local/remote i reconnect. Klucz parowania trafia do Windows Credential
Manager. Wspolne rejestry encji, handlery komend, stany i toasty obsluguja oba
transporty. Dodano UI Settings/Wizard, dokumentacje oraz test zgodnosci na
wektorach wygenerowanych pythonowym kodem integracji HA (tylko odczyt).
Weryfikacja: `cargo check`, `cargo test` (2/2), `npx tsc --noEmit`; `cargo tree`
bez ring/openssl/rustls. Bez testow z zywym HA, bez builda, push i merge.
Dotkniete pliki: Rust/Cargo transportu i konfiguracji, UI TS/TSX, README.md,
docs/LINK.md, generator i publiczny fixture testowy, AGENT-LOG.md.
Nastepny krok: T20 — finalny status obu repo, backup selftest i manualny E2E.

## [2026-07-19] Codex — T20 finalizacja fali 4
Zrobione: finalny backup HomeAssistant wygenerowany z commita `f559ef9` i
przeszedl selftest (2391 plikow, 19 978 240 B, komplet deskmate_link, skan
sekretow). STATUS obu repo zawiera wyniki, decyzje i manualna checkliste E2E.
Strefy Claude'a pozostaly bez zmian. IN PROGRESS wyczyszczone.
Dotkniete pliki w tym repo: STATUS.md, AGENT-LOG.md.
Nastepny krok: Jakub wykonuje E2E na HAOS; dopiero po jego jawnym "tak" mozna
pushowac lub scalac branch.

## [2026-07-19] Codex — T21 Deskmate Link v0.2
Zrobione: Link declare obejmuje cztery warunkowe encje text z nazwami MQTT,
przyciski prezentacji i encje event dla kazdego hotkeya. Nacisniecie hotkeya
wysyla `trigger`/`press` z kluczem `hotkey_<id>`; akcje lokalne/API pozostaja
wykonywane, a dedykowana akcja eventowa publikuje tylko zdarzenie. Kazda zmiana
ustawien encji/hotkeyow wysyla pelny re-declare albo restartuje sesje z pelnym
declare, co pozwala HA v0.2 usunac znikniete encje. UI i docs/LINK.md opisuja
oba transporty oraz parity.
Weryfikacja: `cargo check`, `cargo test` (4/4, w tym niezmienione wektory
Pythona), `npx tsc --noEmit` i `cargo tree` bez ring/openssl/rustls. Bez zywego
HA, builda, push i merge.
Dotkniete pliki: discovery/transport/actions/hotkeys Rust, dwa opisy UI,
docs/LINK.md i AGENT-LOG.md.
Nastepny krok: T22 regeneracja backupu HA, potem T23 statusy i raport.

## [2026-07-19] Codex — T23 finalizacja fali 5
Zrobione: backup HA po integracji Link v0.2 przeszedl selftest (2394 pliki,
19 988 480 B), a DryRun deployu objal event.py, text.py i deskmate_tools.py.
Centralny STATUS zawiera decyzje, znalezione problemy w strefie Claude'a i
checkliste E2E dla text, hotkey trigger, prune oraz narzedzi Jarvisa. IN
PROGRESS wyczyszczone.
Dotkniete pliki w tym repo: AGENT-LOG.md.
Nastepny krok: Jakub wykonuje E2E; push/merge dopiero po jego jawnym "tak".

## [2026-07-19] Codex — T32 sensory sprzetowe
Zrobione: dodano lekkie sensory GPU/VRAM/dyskow/temperatur przez natywne
PDH, DXGI, WMI i istniejace sysinfo. Definicje sa dynamiczne: brak poprawnego
odczytu oznacza brak deklaracji; wykryte sensory trafiaja rownolegle do MQTT
discovery i Link declare, a znikniecie sensora usuwa retained discovery.
Weryfikacja: cargo check, cargo test (6/6), tsc oraz cargo tree bez
ring/openssl/rustls. Bez uruchamiania aplikacji, HA, push i merge.
Dotkniete pliki: hardware/sensors/discovery/transport i petle transportow,
stan aplikacji, feature flags Windows, STATUS.md i AGENT-LOG.md.
Nastepny krok: T34 Link Files v1; wpis IN PROGRESS pozostaje aktywny.

## [2026-07-19] Codex — T34 Link Files v1
Zrobione: dodano read-only fs/fs_res z pusta domyslna allowlista, UI rootow,
canonicalize i ochrona przed traversal/UNC/ADS/reparse point. Read ma limity
256 KiB/chunk, 16 MiB/file i 4 MiB/s; kazda proba trafia do security.log bez
tresci pliku. Testy jednostkowe obejmuja .., UNC/device paths, ADS, sciezke
drive-relative, case-insensitive containment i pusty default.
Weryfikacja: cargo check, cargo test (13/13), tsc i cargo tree bez
ring/openssl/rustls. Bez zywego Link/HA, push i merge.
Dotkniete pliki: link_files.rs, handler Link, config/save, security audit,
Settings/types, docs/LINK.md, STATUS.md i AGENT-LOG.md.
Nastepny krok: T35 finalny tar i raport fali 7.
