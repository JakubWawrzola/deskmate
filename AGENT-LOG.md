# Agent activity log

Protocol: see [AGENTS.md](AGENTS.md). Read the IN PROGRESS table below before
editing anything. Append your own entry after finishing a task — never edit
or delete another agent's entry.

## IN PROGRESS (update before you start, clear when you finish)

| Area | Agent | Since |
|---|---|---|

## Log

## [2026-10-22] Claude — 0.8.3: #9 czujniki sprzetowe, #10 Disconnect
Zrobione: OwnedSensorDef.hardware, event deskmate://sensor-defs, panel Hardware
(All on/off, set_sensors_enabled) na stronie Sensors; config.connection_paused,
set_connection_paused, transport::stop (MQTT "offline" przed rozlaczeniem),
Disconnect/Connect na Status i w trayu, komunikat odrzucenia wspomina
wylaczony wpis w HA. Wersja 0.8.3, docs, release, odpowiedzi w #9 i #10.
Dotkniete pliki: src-tauri/src/{config,hardware,discovery,lib,link,sensors,
transport}.rs, src/{App.tsx,api.ts,types.ts}, src/pages/{Sensors,Status}Page.tsx,
wersje, docs, CHANGELOG.

## [2026-10-16] Claude — 0.8.2: Press keys, #8 na roadmapie
Zrobione: keys.rs (parsowanie "Ctrl+F5", sekwencje "Ctrl+L, Ctrl+C", SendInput,
odmowa przy zablokowanym Windows i dla Ctrl+Alt+Delete), akcja "keys" w
komendach wlasnych (lib.rs, sys_commands.rs, discovery.rs), UI Commands.
Issue #8 (Deskmate jako usluga Windows) - projekt w ROADMAP (Next), odpowiedz
na issue. Wersja 0.8.2 wszedzie, docs, release, laptop zainstalowany.
Dotkniete pliki: src-tauri/src/{keys,lib,sys_commands,discovery,config}.rs,
src/{types.ts,pages/CommandsPage.tsx}, wersje, docs, CHANGELOG, README.

## [2026-10-15] Claude — manifest wtyczki Stream Deck jako szablon (HACS)
Zrobione: streamdeck-plugin/com.homeos.homeassistant.sdPlugin/manifest.json
przeniesiony do streamdeck-plugin/manifest.template.json; rollup (buildStart)
kopiuje go do folderu .sdPlugin, kopia w .gitignore. Powod: hassfest w PR do
hacs/default szuka w calym repo dokladnie jednego pliku *manifest.json.
UWAGA Codex (codex/streamdeck-neo): Twoje zmiany manifestu (wersja 0.2.0,
akcje Deskmate Copy) przy mergu przenies do manifest.template.json; nie
commituj manifest.json w folderze .sdPlugin.

## [2026-10-14] Claude — 0.8.1: mikrofon, urzadzenia dzwieku, zasady rodzinne, zakladki
Zrobione (master, akceptacja Kuby 14.10, commit + push na main): audio.rs (mute i
glosnosc mikrofonu, wybor domyslnego wyjscia/wejscia przez IPolicyConfig),
family.rs (czas ekranu, limit, godziny, blokada programow, ochrona konta:
HKLM + zadanie "Deskmate watchdog" + kopia w Program Files), przycisk Sign out,
integracja: select.py, DeskmateOwnedEntity (wartosci trzymane przez HA na
chronionym koncie), fix "Mute audio". UI: Files, Clipboard, Family, panel Sound.
Docs: GUIDE, SECURITY, LINK, ROADMAP, CHANGELOG, RELEASE-0.8.1, README.
Dotkniete pliki: src-tauri/src/{audio,family,config,discovery,lib,link,security,
sensors,sys_commands,transport,cpu_temperature}.rs, Cargo.toml,
custom_components/deskmate_link/{const,entity,number,sensor,switch,text,select}.py,
src/{App.tsx,api.ts,types.ts}, src/pages/{Files,Clipboard,Family,Sensors,Settings}Page.tsx,
docs/*, CHANGELOG.md, README.md, STATUS.md.
Wdrozone: laptop (ARM64), HAOS (przez Tailscale). Nastepny krok: tag v0.8.1 i release na polecenie Kuby.

## [2026-10-11 ~23:55] Claude — DeskmateTV (branch feat/deskmate-tv)
Zrobione: aplikacja Android TV w `tv/` (Link v2 w Kotlinie z testami
wektorowymi vs crypto.py, usluga dostepnosci: ekran jako tekst, klik, D-pad,
tekst, aplikacje, wejscia HDMI, media, glosnosc, dymki, zrzut, samoaktualizacja,
watchdog). Integracja: wezly typu tv (tv_res, async_tv_request), encja
polaczenia, deskmate_link.tv_update + autoaktualizacja przy polaczeniu,
notify bez node_id tylko do komputerow, kamera jako obraz w dymku TV.
Docs: docs/TV.md, README, CHANGELOG (Unreleased). Wersje aplikacji PC i
manifestu bez zmian (0.7.3) - release to decyzja Jakuba.
Wdrozone u Jakuba: HA (0.7.3 + TV), TV 0.2.0. Push galezi + PR na polecenie
Jakuba z 11.10; bez merge do main, bez tagu i release.
Dotkniete pliki: custom_components/deskmate_link/*, tv/**, docs/TV.md,
README.md, CHANGELOG.md, AGENT-LOG.md.
Nastepny krok: Jakub testuje na TV i decyduje o merge/release.

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

## [2026-07-15 ~16:10] Claude — pojednanie master z origin/main (Kuba: "prawda jest to co jest na githubie")
Kontekst: po publikacji release 0.3.1 lokalny `master` (worktree deskmate) i
`origin/main` na GitHubie mialy ROZNE historie commitow (Codex robil merge +
push w osobnym locie, przez Git Data API, ktory nie wrocil do lokalnego
master w tym worktree). Kuba zdecydowal wprost: GitHub main jest zrodlem
prawdy.
Zrobione: `git fetch origin`, zweryfikowano `git diff --stat master
origin/main` PRZED resetem - jedyna roznica to 3 pliki binarne installerow
(celowo tylko jako GitHub Release assets, nie w repo) - zero roznicy w
kodzie/docsach. Wykonano `git reset --hard origin/main` w worktree
`C:\dev\web\deskmate`. `cargo check` + `npx tsc --noEmit` przeszly na nowym
stanie. Lokalny tag `v0.3.1` pobrany z fetch.
Dotkniete pliki: brak zmian w kodzie, tylko ref brancha master + working tree
(usuniete z dysku: dist-installers/*.exe/*.zip - byly tylko lokalnymi build
artefaktami, sa na GitHub Release jako assets).
UWAGA dla Codexa: `codex/work` w worktree deskmate-codex NADAL wskazuje na
stara, rozjezdzona historie (commit 87378d1) - nie zostal dotkniety. Przy
kolejnej sesji w tym worktree rozwaz `git fetch && git reset --hard
origin/main` tam tez, zeby oba worktree byly zgodne z GitHubem - ale to
robi Codex swiadomie, nie zostalo zrobione automatycznie.
Nastepny krok: brak w toku.

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

## [2026-07-19] Codex — T35 finalizacja fali 7
Zrobione: finalny backup HomeAssistant po commicie Claude'a z piecioma
panelami glass przeszedl selftest (2396 plikow, 20 008 960 B), kontrole
sekretow, czterech zasobow Lovelace i porownanie hashy plikow buildow, motywu
oraz card-mod. STATUS obu repo zawiera decyzje, ograniczenia i manualna
checkliste sensorow oraz Files. Zweryfikowano szkic delegowanego zadania
`[TANI]`; nie przyjeto od niego zmian w repo.
Dotkniete pliki w tym repo: STATUS.md, AGENT-LOG.md.
Nastepny krok: Jakub wykonuje E2E na sprzecie i HAOS; dashboard Komputery
wymaga zmiany Claude'a, a pelny Files E2E serwerowej czesci T36. Bez push,
merge i deployu.

## [2026-07-20] Codex — T41 release-prep Deskmate 0.4.0
Zrobione: podniesiono wersje npm/Cargo/Tauri do 0.4.0, dodano CHANGELOG i
gotowe release notes dla Link, text/hotkeyow, sensorow sprzetowych i read-only
Files v1. Cargo check, 13/13 testow, tsc, JSON oraz cargo tree bez
ring/openssl/rustls przeszly. Zbudowano i sprawdzono NSIS x64+ARM64, wersje
plikow, hashe kopii, ZIP i jego listing. EXE/ZIP pozostaja lokalne;
SHA256SUMS.txt jest zaktualizowany.
Dotkniete pliki: metadane wersji, Cargo.lock, README.md, CHANGELOG.md,
docs/RELEASE-0.4.0.md, STATUS.md, AGENT-LOG.md, dist-installers/SHA256SUMS.txt.
Nastepny krok: manualny upgrade/E2E na x64 i ARM64. Merge, tag, release i push
wylacznie po osobnym `tak` Kuby; niczego nie opublikowano.

## [2026-07-20] Claude — publikacja 0.4.0 na GitHub (polecenie Kuby)
Zrobione: commit wpisu pojednania z 07-15 (dbae113), merge
feature/deskmate-link -> master (2a624f3, konflikt AGENT-LOG.md rozwiazany
zachowaniem OBU wpisow - append-only), README odswiezone pod 0.4.0 (Link,
hw sensors, Files), push HEAD:main (bac7c8b..d100067), tag v0.4.0.
Release v0.4.0 utworzony jako PRERELEASE (brak testu E2E z zywym HA -
uczciwie oznaczone; po zaliczonym E2E na nowym HAOS mozna zdjac flage).
UWAGA dla Codexa: po tym merge'u worktree deskmate-codex wymaga
git fetch && git reset --hard origin/main przy nastepnej sesji.
Nastepny krok: E2E po odbudowie HAOS (checklista w STATUS.md repo
HomeAssistant), potem ewentualnie zdjecie flagi prerelease.

## [2026-09-23] Claude — 0.6.0: Link v2, duplikaty wpisow, stabilne sensory, hardening
Zrobione: protokol Link v2 (X25519 + PSK, MAC transkryptu, ratchet min_version)
po obu stronach, wektory Python->Rust, scalanie zdublowanych wpisow HA,
ponowne uzycie wiszacego wpisu, kod parowania DMP1, powody odrzucenia
(clock/cascade/version), retry PDH + histereza sensorow sprzetowych,
transakcyjny save_config z migracja kluczy, tokeny przyciskow toastu, CSP,
zeroize, polityka ws://, sprzatanie obrazkow, -EncodedCommand. Wersja 0.6.0.
cargo check, cargo test 16/16, tsc, py_compile. Bez builda, commita i pusha.
Dotkniete pliki: src-tauri/src/{link,lib,config,consts,ha_api,hardware,notify}.rs,
Cargo.toml/lock, tauri.conf.json, src/{pairing.ts,pages/Wizard,Settings,Geeky},
custom_components/deskmate_link/*, docs (LINK, SECURITY, AI-DEPLOY,
CODEX-TASKS), README, CHANGELOG, STATUS, package.json; fixture v1 usuniety, v2 dodany.
Nastepny krok: wdrozenie integracji na Pi, potem Deskmate; zgoda Kuby na commit.

## [2026-09-23] Claude — wdrozenie 0.6.0 (Pi + laptop)
Zrobione: build NSIS ARM64 + x64 0.6.0, SHA256SUMS; backup i deploy integracji
0.6.0 na Pi przez SMB/Tailscale, restart Core przez API z obserwacja przerwy;
laptop zainstalowany 0.6.0 i polaczony protokolem v2 (min_version=2). Kopia
integracji w repo HomeAssistant zsynchronizowana. PC nie zainstalowany (poza
siecia Tailscale). Bez commita i pusha.
Dotkniete pliki: dist-installers/SHA256SUMS.txt (+ exe, ignorowane), STATUS.md,
AGENT-LOG.md; HomeAssistant/domos/custom_components/deskmate_link/*.
Nastepny krok: instalacja x64 na PC przez Kube, testy, zgoda na commit.

## [2026-09-23] Claude — commity 0.5.0 i 0.6.0, release v0.6.0 (polecenie Kuby)
Zrobione: stan 0.5.0 odtworzony ze snapshotu 0.6.0 przez odwrocenie podmian
z tej sesji (integracja z backupu na Pi, lib.rs z file-history), zweryfikowany
zgodnoscia git diff --stat z poczatkiem sesji; commit 0.5.0, potem 0.6.0
z docs/RELEASE-0.6.0.md. Bez stopki Co-Authored-By. Push main, tag v0.6.0,
release Latest z assets. Repo HomeAssistant bez commita.
Nastepny krok: instalacja x64 na PC.

## [2026-09-24] Claude — 0.7.0 Link Files v2 (pliki telefon/laptop -> PC przez HA)
Zrobione: skrzynka odbiorcza na PC (off/confirm/automatic, put_begin/chunk/
end/abort, sanityzacja nazw, bez nadpisywania, .part + SHA-256, MotW, toast),
op `roots`, wspolny limit rozmiaru; w HA panel "Deskmate Files" (admin),
API HTTP z uploadem po 8 MiB i pobieraniem przez podpisany URL, uslugi
send_file/fetch_file. Fix: cmd/fs w Linku jako osobne zadania. 20/20 testow.
Wdrozone na Pi (restart Core) i na laptopie. Bez commita i pusha.
Dotkniete pliki: src-tauri/src/{link_files,link,lib,config,security}.rs,
src/{types.ts,pages/SettingsPage.tsx}, custom_components/deskmate_link/
{files.py,hub.py,const.py,__init__.py,manifest.json,services.yaml,frontend/},
docs (LINK, SECURITY), README, CHANGELOG, STATUS, wersje 0.7.0.
Nastepny krok: test Kuby z telefonu, zgoda na commit/release.

## [2026-09-24] Claude — release v0.7.0 (polecenie Kuby)
Zrobione: docs/RELEASE-0.7.0.md, commit 0.7.0 bez Co-Authored-By, push main,
tag v0.7.0, GitHub Release Latest z instalatorami x64/ARM64 i SHA256SUMS.
Repo HomeAssistant bez commita.
Nastepny krok: test Kuby, post na Reddicie.

## [2026-09-24] Claude — PR #1 (type_text) scalony na polecenie Kuby
Zrobione: PR #1 od @anupamme (filtr znakow sterujacych w type_text) sprawdzony
lokalnie (cargo check, 20/20 testow) i scalony squashem na GitHubie (0a1354e,
autor: Anupam Mediratta). Dopisano notke w README, SECURITY.md i CHANGELOG
(Unreleased). Bez nowego release'u.
Nastepny krok: zmiana trafi do kolejnego wydania.


## [2026-10-01] Claude — GitHub issues #2 i #3 (bez commita)
Zrobione: #2 Bitdefender (toasty i skrot Start bez PowerShella: WinRT z wlasnego
XML + IShellLink/IPropertyStore przez COM, usuniety tauri-winrt-notification,
HomeOS -> Deskmate, sprzatanie HomeOS.lnk), okno widgetow (capability widget:
start-dragging + hide), czarny Toggle (left-0), okno na wierzch
(show_main_window + single-instance), sprawdzanie aktualizacji (updates.rs),
integracja w HA nazywa sie "Deskmate", ikona brand/, panel "Deskmate" z
zakladka Computers, jeden przewodnik docs/GUIDE.md, docs/releases + docs/dev,
nowy README. #3: seat.rs (mutex Global\Deskmate.Seat.<node>, oddawanie
polaczenia aktywnej sesji), HA Reconfigure -> Pair another Windows user (kod z
n + c). installMode both wycofany (UAC przy kazdej instalacji), zostaje per-user.
Dotkniete pliki: src-tauri (notify, seat, updates, lib, link, mqtt, actions,
config, consts, Cargo.toml, tauri.conf.json, capabilities/widget.json), src
(pairing, Wizard, SettingsPage, StatusPage, GeekyPage, WidgetPanel, components,
api, types), custom_components/deskmate_link (config_flow, files, hub, const,
manifest, strings, translations, panel JS, brand/), hacs.json, docs, README,
CHANGELOG (Unreleased), HANDOFF.
Wydanie (polecenie Kuby "zrob 7.1, zacommituj, odpowiedz"): wersja 0.7.1,
commit bez Co-Authored-By, tag v0.7.1, GitHub Release, komentarze w #2 i #3.
Integracja na Pi + ARM64 na laptopie. Po deployu naprawiony wyscig rejestracji
panelu (dwa wpisy naraz). Repo HomeAssistant bez commita.
Nastepny krok: testy reczne Kuby, instalacja 0.7.1 na PC x64.

## [2026-10-05] Claude — 0.7.2: sensory kamery i mikrofonu (issue #4)
Przyczyna: aktualne buildy Windows 11 nie zeruja LastUsedTimeStop w
CapabilityAccessManager\ConsentStore, gdy aplikacja nadal uzywa urzadzenia
(to samo w HASS.Agent, PR hass-agent/HASS.Agent#476). Nowy media_activity.rs:
mikrofon = aktywna sesja Core Audio na aktywnym urzadzeniu nagrywajacym,
kamera = IMFSensorActivityMonitor w tle (callback przez #[implement]),
rejestr tylko jako fallback. Sonda read-only na laptopie (26200): oba API
odpowiadaja, monitor wysyla raporty takze bez aktywnosci. Zaleznosc
windows-core 0.61 (juz w drzewie) + feature Win32_Media_MediaFoundation.
Wersja 0.7.2 (bez zmian w integracji HA poza numerem).
Kuba przetestowal i zaakceptowal: commit, push main, tag v0.7.2, release, odpowiedz w #4.

## [2026-10-10] Claude — 0.7.3: druga runda uwag CBY0 (#2)
Przyczyny: (1) menu_options w strings/translations na poziomie config zamiast
step.reconfigure -> puste strzalki w Reconfigure, druga = new_key = rotacja
klucza; (2) UNBOUND_TITLE po polsku, polskie bledy i services.yaml;
(3) deinstalacja Tauri kasuje tylko %APPDATA%\com.deskmate.desktop, a config
jest w %APPDATA%\Deskmate, klucze w Credential Manager, wpisy w HKCU\Classes;
(5) os error 10013 = firewall/antywirus (Bitdefender) blokuje deskmate.exe.
Zrobione: etykiety menu + opcja "Show the pairing code", angielskie teksty HA
(+ tlumaczenie pl, migracja tytulu), uninstall.rs (--forget-this-computer:
Credential Manager po sufiksach + %APPDATA%\Deskmate) i windows/hooks.nsh
(PRE: forget przy "Delete application data", POST: klucze rejestru),
transport::explain_connection_error (10013/10061/10060/10065), panel API HA,
GUIDE (10013, reinstalacja, pelny reset + nazwy w Credential Manager).
Sonda read-only: filtr Credential Manager trafia 3/13 wpisow, tylko Deskmate.
Kuba zaakceptowal: commit, push main, tag v0.7.3, release, odpowiedz w #2.

## [2026-10-13] Claude — merge DeskmateTV + feedback spolecznosci (master, bez push)
Merge feat/deskmate-tv -> master (fast-forward). TV jako preview: tv_update
tylko przy sparowanym TV, README bez sekcji TV, docs/TV.md oznaczony, roadmapa
(TV: powiadomienia z podgladem kamery; aplikacja na Linuxa). Funkcje: Link
domyslny dla nowych instalacji (brak pola transport = MQTT), #7 autostart
zapamietany w configu i odtwarzany, #5 ciemny motyw + Last active + wlasne
sensory (proces, okno, usluga, PowerShell) + sterowanie programami (start/stop
bez PowerShella), dashboard HA w oknie aplikacji, temperatura CPU przez
PawnIO (helper w Program Files, zadanie przy logowaniu, named pipe, moduly
0.2.11 LGPL w src-tauri/pawnio). cargo check ARM64 i x64 (--tests), tsc OK.
Nie budowano, nie testowano na urzadzeniach, nie pushowano. Wersja nadal 0.7.3.
Wydanie (polecenie Kuby 13.10): wersja 0.8.0, docs uzupelnione (LINK parity,
ROADMAP, RELEASE-0.8.0), commit bez Co-Authored-By, push main, tag v0.8.0,
release z instalatorami, odpowiedzi w #5 i #7.
Nastepny krok: testy Kuby (helper PawnIO na PC x64).
