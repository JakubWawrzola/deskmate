# Roadmap

Kolejnosc wg wartosci dla uzytkownika; nic z tej listy nie blokuje wydania 0.1.

## Toast reliability — status 0.2.1
- [x] Fallback: gdy in-process WinRT `.show()` zawiedzie, toast leci przez swiezy
      proces PowerShell (POWERSHELL_APP_ID, CREATE_NO_WINDOW). Rozwiazuje "przycisk
      nic nie robi" bez zmiany brandingu. Przyciski akcji dzialaja tylko gdy in-process OK.
- [x] "Send test toast" raportuje wynik w UI (sukces + podpowiedz o Focus, albo blad).

## Toast branding (dlug techniczny z 0.1.2, proba w 0.2 nieudana)
- [ ] Toasty pokazuja sie jako "Windows PowerShell" (uzywamy POWERSHELL_APP_ID,
      bo wlasny AUMID wymaga skrotu z AppUserModelID w Menu Start). Proba w 0.2
      przez COM (IShellLink+IPropertyStore) odlozona: windows crate 0.61 nie
      eksponuje InitPropVariantFromString/PROPERTYKEY w spodziewanej sciezce.
      Najprostsza droga: hook instalatora NSIS (plugin ApplicationID / WinShell)
      ustawia System.AppUserModel.ID na skrocie przy instalacji; wtedy w
      notify.rs ustawic BRANDED=true (albo bezwarunkowo TOAST_AUMID). Kod flagi
      BRANDED + ensure_branding() juz jest - wystarczy zrodlo skrotu.
      Alternatywa: pakiet MSIX. Runtime dziala bez tego (PowerShell AUMID).

## 0.2 — jakosc zycia
- [ ] Broker TLS (mqtts://) — rustls, gdy toolchain ARM64 nie bedzie wymagal clanga (aws-lc-rs prebuilt albo ring z clangiem w CI)
- [ ] Toast: przyciski akcji (odpowiedz zwrotna do HA przez MQTT topic `.../notify/action`)
- [ ] Ikona traya odzwierciedlajaca stan polaczenia (dwa warianty PNG)
- [ ] Autodetekcja brokera (mDNS `_mqtt._tcp`)
- [ ] i18n UI (EN/PL)

## 0.3 — schowek i pliki (pomysl Jakuba)
- [ ] "Clipboard bridge": tekst z PC publikowany retained na `deskmate/<node>/clipboard`
      (sensor w HA, karta z przyciskiem kopiuj na telefonie), wklejanie z HA na PC
- [ ] Przesylanie plikow PC -> HA: upload do `www/deskmate/` przez long-lived token
      (opt-in), link jako powiadomienie na telefonie
- [ ] Kierunek odwrotny: HA -> PC (pobranie pliku z URL do folderu Downloads, toast)

## 0.4 — media i wieksza kontrola
- [ ] Pelna encja `media_player` (wymaga malej custom integration HA albo HACS
      mqtt-mediaplayer — decyzja po feedbacku)
- [ ] Okladka utworu (SMTC thumbnail) jako `image` entity
- [ ] Sensor kamera/mikrofon w uzyciu (capability access manager) — privacy, opt-in
- [ ] Screenshot na zadanie (opt-in, powiadomienie przy kazdym uzyciu)

## 0.5 — Stream Deck
- [ ] Wg docs/STREAMDECK-PLAN.md

## Infrastruktura
- [ ] GitHub Actions: build x64 + ARM64, release artifacts
- [ ] Code signing (koszt — decyzja Jakuba)
- [ ] Tauri updater (auto-aktualizacje)
