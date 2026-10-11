# DeskmateTV (Android TV app)

Gives a Home Assistant assistant full control of an Android / Google TV over
[Deskmate Link](../docs/LINK.md): the screen as compact text, clicks, D-pad,
typing, apps and deep links, HDMI inputs, now playing, volume, on-screen
bubbles and a screenshot on request. User documentation: [docs/TV.md](../docs/TV.md).

## Layout

```
app/src/main/java/com/wawrzola/deskmatetv/
  link/       LinkCrypto (X25519, HKDF, AES-GCM, ChaCha20 cascade), LinkClient (OkHttp WebSocket, backoff)
  tv/         ScreenReader (tree -> text), Actions, Guard (confirmation policy),
              Apps, Inputs, Media + Volume, Screenshot, Overlay
  service/    TvAccessibilityService, MediaListenerService, Watchdog (job + boot),
              ControlReceiver (PROVISION / STATUS / RECONNECT over ADB)
  update/     Updater (self-update over Link)
  ui/         StatusActivity (diagnostics screen)
  Agent.kt    connection, states and entity declaration for Home Assistant
  Ops.kt      the {"t":"tv","op":...} operations
app/src/test/ LinkCryptoTest: vectors from ../custom_components/deskmate_link/crypto.py
tools/gen_vectors.py   vector generator
scripts/build.ps1      tests + signed release into dist\
scripts/install.ps1    ADB install, permissions, pairing, checks
```

Kotlin, no AndroidX. The only dependency is OkHttp. `minSdk` 28, `targetSdk` 35.

## Build

Needs JDK 17+, the Android SDK (platform 35) in `local.properties` and Python
with `cryptography` for the test vectors.

```powershell
.\scripts\build.ps1
```

The release is signed with the key from `_prywatne\keystore.properties`
(`storeFile`, `storePassword`, `keyAlias`, `keyPassword`). That folder is not
committed. Create a key once with `keytool -genkeypair` and keep a backup:
updates only install over a build signed with the same certificate.
