# Stream Deck integration — plan (NOT implemented)

Cel: dodawanie encji HA na klawisze Elgato Stream Decka bez recznego
konfigurowania pluginow websocketowych.

## Ocena wykonalnosci (2026-07-10)

Da sie, ale to osobny, niemaly modul. Swiadomie odlozone: wymaga SDK Elgato
(plugin = osobny proces Node/WebSocket na porcie 28196 zarzadzany przez
aplikacje Stream Deck), wlasnego manifestu i cyklu release'owego. Nie blokuje
zadnej innej funkcji Deskmate.

## Docelowa architektura

```
Stream Deck app ── uruchamia ──> deskmate-streamdeck plugin (Node, SD SDK v2)
                                    │ localhost WS (port konfigurowalny)
                                    ▼
                              Deskmate (Rust core)
                                    │ MQTT (istniejace polaczenie)
                                    ▼
                              Home Assistant
```

Plugin NIE laczy sie z HA sam — gada z lokalnym Deskmate, ktory juz ma
polaczenie, autoryzacje i cache stanow. Dzieki temu:
- jedna konfiguracja (broker w Deskmate),
- klawisze reaguja na stany (ikona odzwierciedla on/off encji),
- brak drugiego kanalu do zabezpieczenia.

## Kroki implementacji (przyszla sesja)

1. **Deskmate: lokalny kanal** — WebSocket server na 127.0.0.1 (port w
   configu, token w Credential Manager). API: `list_entities` (cache z HA:
   wymaga subskrypcji `homeassistant/statestream` ALBO rozszerzenia o REST
   token HA — decyzja przy implementacji), `call` (press/toggle), `subscribe`.
2. **Plugin SD** (TypeScript, @elgato/streamdeck): akcja "HA Entity" z
   property inspectorem — dropdown encji pobierany z Deskmate, wybor akcji
   (toggle/press/scene), render stanu na klawiszu (SVG monochrom).
3. **Instalacja**: Deskmate wykrywa zainstalowany Stream Deck
   (`%APPDATA%\Elgato\StreamDeck`), przycisk "Install Stream Deck plugin"
   kopiuje `.sdPlugin` i restartuje SD.
4. **Testy manualne**: klawisz togguje swiatlo; stan klawisza nadaza za
   zmiana z innego zrodla; brak SD = brak zmian w Deskmate.

## Ryzyka

- SD SDK wymaga Node runtime w pluginie (Elgato dostarcza), ARM64: Stream
  Deck app dziala przez emulacje x64 — do sprawdzenia na zenbooku.
- Lista encji: MQTT discovery nie daje stanow innych encji HA; najprostsza
  poprawna droga = opcjonalny long-lived token HA (REST /api/states +
  websocket) TYLKO dla tej funkcji. Zgoda usera w UI, token w Credential
  Manager.
