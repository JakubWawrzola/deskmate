# STATUS — Deskmate
Aktualizacja: 2026-09-24 (0.7.0 wydane: commit, tag v0.7.0, release; PC czeka na instalacje)

## Sesja 2026-09-24 — 0.7.0: Link Files v2 (PLAN, odhaczane na biezaco)

Cel Kuby: szybkie kopiowanie plikow z laptopa/telefonu na PC przez Home
Assistant, Deskmate jako bezpieczny most. Dotad: tylko odczyt (fs list/stat/
read) po stronie Deskmate, w HA brak jakiegokolwiek UI/uslug.

Projekt:
- Deskmate: skrzynka odbiorcza (inbox) tylko do zapisu, osobna od folderow
  do odczytu. Tryb off/confirm/automatic (domyslnie off), folder domyslnie
  %USERPROFILE%\Downloads\Deskmate, limit rozmiaru (wspolny z odczytem,
  domyslnie 256 MiB). Operacje fs: roots, put_begin/put_chunk/put_end/
  put_abort. Nazwa pliku sanityzowana, bez nadpisywania ("x (1).pdf"),
  zapis do .part i rename, SHA-256 weryfikowane na koncu, Mark-of-the-Web
  (Zone.Identifier) na odebranych plikach, toast po odebraniu, audyt.
- HA: panel w pasku bocznym "Deskmate Files" (tylko admin): wybor komputera,
  wysylanie plikow (drag&drop / wybor z telefonu, postep), przegladanie
  folderow z allowlisty i pobieranie. API HTTP z autoryzacja HA, upload w
  kawalkach po 8 MiB (limit Cloudflare 100 MB), pobieranie strumieniowe przez
  podpisany URL. Uslugi deskmate_link.send_file i deskmate_link.fetch_file
  do automatyzacji (tylko sciezki z allowlist_external_dirs).

Kroki:
- [x] 1. Rust: config + link_files (inbox, put*, roots) + link.rs + save_config
- [x] 2. UI Deskmate: ustawienia skrzynki w Settings
- [x] 3. HA: hub (put*/roots), files.py (API + sesje uploadu + uslugi), panel JS, manifest
- [x] 4. Testy jednostkowe Rust, py_compile, tsc
- [x] 5. Dokumentacja (LINK, SECURITY, CHANGELOG, README), wersja 0.7.0
- [x] 6. Build, deploy na Pi i laptop, raport

Wynik: cargo check czysto, cargo test 20/20, tsc, py_compile, node --check.
Pi: backup `/config/deskmate_link_bak_20260924_v060`, integracja 0.7.0 (22 pliki,
hashe zgodne), restart Core (przerwa 5 s -> 37 s). API /api/deskmate_link/files
odpowiada, panel JS serwowany. Laptop 0.7.0 zainstalowany, odpowiada na `roots`
(skrzynka off, root C:\dev\web). PC nie ruszany (poza Tailscale).
Instalatory 0.7.0: ARM64 92EB2AEB...57BA, x64 FBC7991E...8838.
Poprawka przy okazji: cmd i fs w Linku ida jako osobne zadania - okno
potwierdzenia nie blokuje juz sesji (wczesniej brak pongow -> zerwanie).
UWAGA: laptop udostepnia do odczytu caly C:\dev\web (w tym HomeAssistant/_prywatne
z tokenem HA) - do zawezenia przez Kube.
PUBLIKACJA (polecenie Kuby): commit 0.7.0 na main, tag v0.7.0, GitHub Release
0.7.0 jako Latest z instalatorami. Opis: docs/RELEASE-0.7.0.md.
Nastepny krok: Kuba wlacza Receive files i testuje panel z telefonu; nowy post
na r/homeassistant.


## Sesja 2026-09-23 — 0.6.0: Link v2, duplikaty, PC, bezpieczenstwo

Stan GitHuba przed sesja: `origin/main` = lokalny `master` (917f211). Cala 0.5.0
i `custom_components/` + `hacs.json` istnialy TYLKO lokalnie - HACS i przycisk
w README nie mogly dzialac. Release v0.4.0 jest prerelease, "Latest" to v0.3.1.

Zgloszenia Kuby -> przyczyny -> poprawki:
- Powielanie po bledzie parowania: kazde "Dodaj integracje" tworzylo nowy
  wiszacy wpis; nowy wpis przypinal sie do tego samego node'a co stary ->
  dwa wpisy, jedno urzadzenie, encje `_2`. Teraz: flow pokazuje kod istniejacego
  wiszacego wpisu; wpis, ktorym komputer sie laczy, przejmuje encje starych
  (entity_id i historia zostaja) i usuwa je (`hub._absorb_duplicates`).
- Polaczenie na PC: (a) encje GPU/temperatur znikaly przy kazdym nieudanym
  odczycie PDH/WMI (retry PDH_MORE_DATA + histereza 10 tickow), (b) rozjechany
  zegar dawal "zly klucz" (teraz powod `clock`), (c) niezgodna kaskada tez
  wygladala jak zly klucz (powod `cascade`).
- Ciezka instalacja: kod parowania `DMP1.` (klucz + adresy z get_url), wklejany
  w kreatorze/ustawieniach; brakujace tlumaczenie en kaskady; instrukcja HACS.

Link v2 (obie strony): X25519 efemeryczny + PSK w HKDF (forward secrecy), MAC nad
calym transkryptem z kodowaniem z prefiksem dlugosci, ratchet `min_version`
w wpisie (v1 do pierwszego polaczenia v2). Klient mowi tylko v2 ->
**NAJPIERW integracja na Pi, potem Deskmate**.

Inne: tokeny jednorazowe w przyciskach toastu (protokol deskmate:), CSP,
zeroize kluczy, ws:// tylko dla LAN/Tailscale, cascade != psk, save_config
waliduje przed zapisem i przenosi klucze/token HA przy zmianie node_id,
obrazki toastow kasowane po 10 min, branding przez -EncodedCommand.

Weryfikacja: cargo check (0 ostrzezen), cargo test 16/16 (w tym wektory
Python->Rust v2), tsc, py_compile, symulacja huba v2 z atrapami HA.
NIE: build installerow, test z zywym HA, commit, push.
Usuniety `src-tauri/tests/fixtures/deskmate_link_v1.json` (klient nie mowi v1).
Kopia integracji w repo HomeAssistant NIE zostala zsynchronizowana.

WDROZENIE (2026-09-23, Claude, na polecenie Kuby):
- Pi (SMB przez Tailscale 100.106.86.21, sesja net use otwarta przez Kube):
  backup `/config/deskmate_link_bak_20260923/` + `.storage/core.{config_entries,
  entity_registry,device_registry}.bak_20260923`; integracja 0.6.0 wgrana
  (20 plikow, hashe zgodne, __pycache__ skasowany), restart Core przez API
  (przerwa 3 s -> 35 s zaobserwowana). Oba wpisy `loaded`.
- Laptop na 0.5.0 (v1) polaczyl sie z nowa integracja -> zgodnosc wsteczna OK.
- Laptop: zainstalowany 0.6.0 ARM64 (/S), polaczony v2, wpis `laptopwawrzola`
  ma `min_version=2`, 39/39 encji dostepnych, 0 duplikatow.
- PC: NIE zainstalowany (poza Tailscale). Wpis `kuba` bez min_version (akceptuje
  v1 i v2). Instalator: `dist-installers/Deskmate_0.6.0_x64-setup.exe`.
- Instalatory 0.6.0: ARM64 2 782 046 B `37852BAF...43EC5`, x64 3 195 793 B
  `597BDDDE...CA9EC` (SHA256SUMS.txt zaktualizowany). ZIP nie budowany.
- Kopia integracji w repo HomeAssistant (`domos/custom_components/deskmate_link`)
  zsynchronizowana, niezacommitowana.
- Rollback Pi: przywrocic `deskmate_link_bak_20260923` do custom_components
  + ewentualnie `.storage/*.bak_20260923`, restart Core.

PUBLIKACJA (na polecenie Kuby): dwa osobne commity na master - 0.5.0
(odtworzony dokladnie: git diff --stat zgodny co do pliku z poczatkiem sesji,
27 plikow +1038/-100) i 0.6.0; push na main, tag v0.6.0, GitHub Release 0.6.0
jako Latest z instalatorami x64/ARM64 i SHA256SUMS. Opis wydania i najszybsza
sciezka wdrozenia: docs/RELEASE-0.6.0.md. Repo HomeAssistant NIE commitowane.

Nastepny krok: Kuba instaluje x64 na PC i przechodzi checkliste z raportu.

## Sesja 2026-07-31 — 0.5.0: toasty, HACS, kaskada, dokumentacja

WSZYSTKO GOTOWE DO TESTU. Bez commita, bez pusha, bez wydania na GitHubie -
Kuba testuje na laptopie i dopiero potem daje zgode.

Naprawione:
- [x] PRZYCISKI TOASTU. **DWIE niezalezne przyczyny naraz** - pierwsza naprawa
  nie wystarczyla i Kuba zglosil brak przyciskow mimo 0.5.0:
  1. Windows wycina `<actions>`, jesli AUMID nie wskazuje zarejestrowanego
     activatora COM. `ensure_aumid_registered` zapisuje teraz `CustomActivator`
     + `CLSID\LocalServer32`, skrot Start Menu dostaje `ToastActivatorCLSID`
     (VT_CLSID, pid 26), a `-ToastActivated`/`-Embedding` konczy proces bez
     otwierania okna.
  2. `tauri-winrt-notification` 0.8 w `create_template` (src/lib.rs:682-690)
     tworzy `<action>`, ustawia atrybuty i **nigdy nie robi
     `xml_el_actions.AppendChild`** - kazdy toast przez crate mial pusta liste
     akcji. Po naprawie punktu 1 in-process zaczal sie udawac, wiec toasty
     poszly wlasnie ta zepsuta sciezka. Fix: toast z akcjami omija crate i idzie
     wlasnym XML-em (`show_toast_powershell`), ktory zostal potwierdzony recznie
     - wyslany tak toast pokazal przyciski u Kuby.
- [x] Skrot Start Menu przestal byc jednorazowy: stempel w rejestrze wymusza
  przepisanie po zmianie AUMID/CLSID/sciezki. Stary `HomeOS.lnk` mial AUMID
  `HomeOS.Deskmate`, ktorego nie ma w rejestrze - branding dzialal wylacznie
  dzieki `DisplayName`, nie dzieki skrotowi.

Dodane:
- [x] Kaskadowe szyfrowanie: ChaCha20-Poly1305 nad AES-256-GCM, osobny klucz,
  osobne etykiety HKDF i AAD, osobny wpis w Credential Managerze. Negocjacja
  przez pole `casc` w hello; niezgodnosc = odrzucenie, nigdy downgrade.
- [x] Zakladka **Geeky stuff** (GeekyPage.tsx) z kaskada i opisem tego, co
  faktycznie idzie po drucie.
- [x] Encje: `presenting` (SHQueryUserNotificationState) i switch `audio_mute`
  (IAudioEndpointVolume GetMute/SetMute), oba transporty.
- [x] Kreator startuje od Deskmate Link (istniejaca konfiguracja bez zmian).
- [x] `custom_components/deskmate_link/` + `hacs.json` w repo deskmate =
  instalacja przez HACS, przycisk "Add to HACS" w README.
- [x] Dokumentacja EN: AI-DEPLOY.md (instrukcje dla asystenta AI), MIGRATION.md
  (MQTT -> Link), RELEASE-0.5.0.md, CHANGELOG 0.5.0, przepisany README,
  sekcja Link w SECURITY.md, kaskada w LINK.md.
- [x] CODEX-TASKS.md: C1 (prawdziwy COM activator, ODDANE Codexowi), C2
  (wektory testowe kaskady), C3 (CI: buildy + hassfest/HACS).

Stan wdrozenia i weryfikacji:
- Integracja **0.5.0 WDROZONA na Pi**, Core zrestartowany (przerwa
  potwierdzona obserwacja). Laptop: available, 37 encji. Pecet: WYLACZONY,
  a mimo to **43 encje** - dowod, ze trwalosc `declare` dziala.
- Instalatory 0.5.0 x64 (3 150 011 B) i ARM64 (2 738 781 B) + SHA256SUMS +
  ZIP w `dist-installers/`, przebudowane po poprawce crate'a.
  SHA-256: x64 `404BE9AA...FA767`, ARM64 `E4890C49...0FB3B`.
- `cargo check`, `cargo tree` (bez ring/openssl/rustls), `npx tsc --noEmit`,
  `py -m py_compile` - wszystko zielone.

ZRODLO PRAWDY dla integracji to teraz `deskmate/custom_components/
deskmate_link/`. Kopia w repo HomeAssistant jest robocza i zostala
zsynchronizowana.

## Sesja 2026-07-30 — diagnoza peceta i fala A+B Linka

## Sesja 2026-07-30 — diagnoza peceta i fala A+B Linka

PROBLEM: pecet nie laczyl sie od 28.07. Deskmate wysylal `node=kuba` (domyslny
node_id = zsanityzowany hostname), a wpis w HA byl sparowany recznie wpisana
nazwa `pckuba`. Serwer zamykal gniazdo bez odpowiedzi -> klient pokazywal tylko
`connection closed before welcome`, 2683 warningi w logu HA, staly lockout.
Siec byla sprawna (pecet dobijal sie i po LAN, i przez tunel).

Strona HA (`deskmate_link` 0.2.0 -> 0.3.0, WDROZONE na Pi, Core zrestartowany):
- [x] `reject` z powodem (`auth`/`locked`) zamiast cichego zamkniecia.
- [x] Zgloszenie w Naprawach z nazwa node'a, ktory sie dobija, i instrukcja.
- [x] Throttling logu 1/min na (node, IP) z licznikiem pominietych.
- [x] Lockout per (node, IP) + osobna zapora 100/IP; slownik nie rosnie.
- [x] Trwalosc `declare` w `.storage/deskmate_link.<entry_id>` - encje istnieja
  zaraz po restarcie HA (odpowiednik retained discovery, warunek konieczny dla
  Linka jako glownego transportu).
- [x] Config flow nie pyta juz o node_id; wpis przypina sie do pierwszego
  klienta z poprawnym kluczem. Rekonfiguracja: rotacja klucza / odpiecie.
- [x] Handshake bez efektow ubocznych + odrzucanie powtorzonej `cn`
  (odtworzone `hello` nie wywroci juz zywej sesji).

Strona Deskmate (KOD, bez builda):
- [x] Rozroznienie bledow: odrzucenie / lockout (HTTP 429) / transport.
- [x] Osobny backoff po bledzie autoryzacji: 5/15/30/60 s zamiast co 2 s.
- [x] Status pokazuje node_id przy odrzuceniu; Settings tlumaczy nowy przebieg
  parowania; poprawiony blednie sugerowany wzor entity_id na Status.
- [x] `cargo check` i `npx tsc --noEmit` EXIT=0.

Wpis `pckuba` zostal ODPIETY (node_id="", klucz nietkniety, backup
`.storage/core.config_entries.bak_20260730`) i pecet PRZYPIAL SIE SAM:
node_id `kuba`, 43 encje, device `KubaPC` -> entity_id `*.kubapc_*`
(NIE `kuba_*`: slug bierze sie z device_name, nie z node_id). Klucz zapisany
na pececie byl wiec poprawny - jedynym problemem byla nazwa node'a.
Log HA wyczyszczony. NIE budowano installerow, NIE commitowano.

Poboczne obserwacje z prodowego HA (NIE ruszane, poza zakresem):
- rejestracja 43 nowych encji wywolala `AlexaApiNeedsRelinkError` z Nabu Casa
  (token Alexy do odnowienia),
- `Failed to load services.yaml for integration: domos`,
- 5 zgloszen w Naprawach sprzed tej sesji (google reauth, 3 automatyzacje
  wolajace nieistniejace uslugi esphome/notify, hassio).

## Sesja 2026-07-19 — Deskmate Link (fala odbudowy HAOS 4)

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

Po sesji 2026-07-31: Kuba instaluje `dist-installers/Deskmate_0.5.0_*-setup.exe`
na laptopie i przechodzi checkliste "DO PRZETESTOWANIA - 0.5.0" nizej. Dopiero
po jego zgodzie: commit, tag `v0.5.0`, push i GitHub Release z opisem z
`docs/RELEASE-0.5.0.md`. Tozsamosc commitow: JakubWawrzola /
kontakt@wawrzola.com, bez stopki co-author.

### DO PRZETESTOWANIA - 0.5.0

1. Zainstalowac 0.5.0 na laptopie (ARM64). Polaczenie ma wstac samo, bez
   ponownego parowania - klucz zostaje w Credential Managerze.
2. **Przyciski toastu** (najwazniejsze, dwie proby naprawy za nami):
   Powiadomienia -> Send test toast. Maja byc dwa przyciski pod trescia,
   etykieta "HomeOS". Klikniecie ma dac zdarzenie `deskmate_link_notify_action`
   w HA (Narzedzia deweloperskie -> Zdarzenia). Przy pierwszym kliknieciu
   Windows moze zapytac o skojarzenie protokolu `deskmate:` - potwierdzic.
   Jesli przyciskow nadal nie ma: skasowac `%AppData%\Microsoft\Windows\
   Start Menu\Programs\HomeOS.lnk` i uruchomic Deskmate ponownie.
3. Nowe encje: `binary_sensor.*_presenting_or_full_screen` (wlaczyc pelny ekran
   albo film) i `switch.*_mute_audio` (przelaczyc z HA i recznie na laptopie -
   stan ma nadazac w obie strony).
4. **Kaskada**: HA -> wpis Deskmate Link -> Skonfiguruj ponownie -> Wlacz
   szyfrowanie kaskadowe, skopiowac klucz. W Deskmate: Geeky stuff -> wkleic
   klucz, wlaczyc przelacznik, zapisac. Polaczenie ma wrocic. Potem sprawdzic
   NIEZGODNOSC: wylaczyc kaskade tylko w HA - polaczenie ma padac z komunikatem
   o odrzuceniu, nie wracac po cichu do jednej warstwy.
5. **Komunikaty bledow**: wpisac zly klucz parowania w Deskmate. Status ma
   pokazac `Link rejected ... (node "...")`, w HA ma sie pojawic zgloszenie w
   Naprawach, a ponowne proby maja zwalniac (5/15/30/60 s), nie leciec co 2 s.
6. Zakladka Geeky stuff: opis "What is actually on the wire" ma zmieniac linijke
   Frames po wlaczeniu kaskady.
7. Kreator na czystej instalacji (opcjonalnie, maszyna wirtualna): ma startowac
   od "Deskmate Link (recommended)".

Starsze pozycje: patrz nizej.

Po sesji 2026-07-30 pierwsze w kolejce:
1. Zbudowac i zainstalowac klienta z fali A (nowe komunikaty bledow + backoff)
   - dopiero wtedy da sie zobaczyc `Link rejected`/`Link locked out`.
   Do tego czasu pecet i laptop chodza na 0.4.0 i dzialaja (serwer 0.3.0 jest
   wstecznie zgodny - potwierdzone na obu maszynach).
2. Dashboard `HomeAssistant/dashboards/komputery.yaml` ma nadal zaslepke
   onboardingu dla PC. Encje juz istnieja jako `*.kubapc_*` - do podmiany.
3. Decyzja: czy `device_name` peceta ma zostac `KubaPC`. Zmiana teraz NIE
   przemianuje istniejacych encji, trzeba by je usunac z rejestru.

Starsze, wciaz czeka na Kube:
- przetestowac lokalnie installery 0.4.0 x64 i ARM64 wedlug checklisty T41
- po zaliczonym E2E osobno zdecydowac o merge i publikacji; opis jest gotowy
  w `docs/RELEASE-0.4.0.md`, ale bez jawnego `tak` nic nie publikowac
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
  `C:\dev\web\deskmate-codex` (`feature/deskmate-link`). Protokol: `AGENTS.md`.
- Archiwum 0.2.3: `C:\dev\web\deskmate-0.2.3-archiwum` - NIE RUSZAC.
- Wersja robocza: 0.4.0 release-prep; opublikowana wersja pozostaje 0.3.1.
- Build: npx tauri build --target x86_64-pc-windows-msvc / aarch64-pc-windows-msvc
- node_id laptopa Kuby: laptopwawrzola. Stare adresy brokera/HA wymagaja
  zastapienia adresami zapisanymi po odbudowie sprzetu.

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

## Fala 9 - T41 release-prep 0.4.0

Status: wykonane offline; merge, publikacja i E2E wymagaja osobnej zgody/testu

- Wersje package/npm, Cargo i Tauri podniesiono do 0.4.0. Dodano CHANGELOG,
  release notes 0.4.0 i wskazanie nowego opisu w README.
- `cargo check`, `cargo test` (13/13), `npx tsc --noEmit`, parsowanie JSON i
  `cargo tree` (860 linii, bez ring/openssl/rustls) przeszly.
- Zbudowano NSIS x64 (3 133 750 B) i ARM64 (2 728 037 B). Oba pliki maja
  ProductVersion i FileVersion 0.4.0; hashe kopii w `dist-installers/` sa
  identyczne z artefaktami Tauri.
- `SHA256SUMS.txt` zawiera:
  - x64: `29D85276A8E85216D099F4F46841E2014CF8797D452F80A0F2904C7D5E13494B`;
  - ARM64: `EBAE8D45A0149734CE12F3D93B925DAD0E79D0882DC2075B9965519D7CF44A13`.
- `Deskmate_0.4.0_installers.zip` ma 5 825 504 B i SHA-256
  `6C22201B654585B2DB91B16E6095FE66630235FA3AE83C08546EE3DA0D669D02`;
  listing zawiera dokladnie oba installery i `SHA256SUMS.txt`.
- EXE i ZIP pozostaja lokalnymi, niesledzonymi artefaktami. Nie wykonano
  instalacji, polaczenia z HA/MQTT/Link, merge, tagu, release ani push.

### DO PRZETESTOWANIA - Deskmate 0.4.0

1. Wykonac upgrade z 0.3.1 na 0.4.0 osobno instalatorem x64 i ARM64;
   potwierdzic zachowanie konfiguracji i danych Windows Credential Manager.
2. Potwierdzic, ze MQTT pozostaje transportem domyslnym i discovery nadal
   deklaruje dotychczasowe encje.
3. Sparowac Link i sprawdzic sensory, text, hotkeye/eventy oraz prune po
   zmianie konfiguracji.
4. Sprawdzic sensory GPU/VRAM/dysk/temperatury na realnym sprzecie; brak
   wiarygodnego odczytu nie moze utworzyc encji.
5. Przy pustej allowliscie Files oczekiwac odmowy. Po dodaniu katalogu
   testowego sprawdzic list/stat/read oraz odmowy dla `..`, UNC, ADS i reparse.
6. Dopiero po zaliczonym E2E Kuba moze osobno zgodzic sie na merge, tag,
   publikacje GitHub Release i push.
