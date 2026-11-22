# Contributing

Thanks for helping. Bug reports and ideas are as welcome as code.

## Reporting a bug

Open an [issue](https://github.com/JakubWawrzola/deskmate/issues/new/choose)
with the bug form. Most useful:

- the Deskmate version on the PC and the integration version in Home Assistant
  (HACS shows it);
- Windows version and whether the PC is x64 or ARM64;
- the text on Deskmate's Status page;
- from Home Assistant: the computer's entry → three dots → *Download
  diagnostics*. Keys are removed from it.

Never paste a pairing code, a Home Assistant token or a password. Security
problems go through [private reporting](.github/SECURITY.md), not an issue.

## Asking for a feature

Use the feature form and describe what you want to do at home, not only the
setting you imagine. Questions and "how do I" go to
[Discussions](https://github.com/JakubWawrzola/deskmate/discussions).

## Changing the code

What lives where:

| Folder | What |
|---|---|
| `src-tauri/` | The desktop app for Windows and Linux, the shared `deskmate-core` and the `deskmate-service` crate (Windows service, Linux daemon): Rust (Tauri 2) |
| `src-tauri/crates/core/` | `deskmate-core`: the part of the app that needs neither Tauri nor a signed-in user, the Link protocol included |
| `src/` | The app's window: React and TypeScript |
| `custom_components/deskmate_link/` | The Home Assistant integration (Python) |
| `streamdeck-plugin/` | The Stream Deck plugin |
| `tv/` | DeskmateTV for Android TV (preview) |
| `docs/` | The guide, protocol and security notes |

Building:

```
npm install
npm run tauri dev
```

Toolchain: Node 20+, Rust stable, the `x86_64-pc-windows-msvc` or
`aarch64-pc-windows-msvc` target.

Turn on the pre-commit hook once. It keeps key and token files out of commits
and scans what you stage with [gitleaks](https://github.com/gitleaks/gitleaks),
which has to be installed:

```
git config core.hooksPath .githooks
```

Before a pull request, the same checks CI runs:

- in `src-tauri`: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace`;
- in the root: `npx tsc --noEmit` and `npm run lint` (`npm run format` fixes
  the formatting);
- for the integration: `ruff check` and `ruff format --check` on
  `custom_components scripts tests`, and `python -m pytest` on Linux or in
  WSL, in a virtual environment built from `requirements-test.txt`; the HACS
  and hassfest checks of this repository stay green;
- user-facing text in plain English, without jargon;
- docs updated when behaviour changes (`docs/GUIDE.md`, `CHANGELOG.md`).

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org)
(`feat:`, `fix:`, `docs:`, ...). Keep one topic per pull request.

## Improving a translation

Deskmate speaks English, German, Spanish, French, Polish, Brazilian
Portuguese and Simplified Chinese. Most translations were not written by
native speakers, so corrections are welcome and need no programming:

- the window: `src/i18n/<language>.json`, one line per text, the English
  text on the left and the translation on the right;
- the tray menu and the notifications: `src-tauri/i18n/<language>.json`;
- the Home Assistant integration:
  `custom_components/deskmate_link/translations/<language>.json`.

Change only the right-hand side. Keep the words in braces, such as
`{version}`, exactly as they are, and keep a space at the start or end of a
text where the English one has it: some texts are parts of a sentence. The
checks tell you when one of these slipped. A pull request with the corrected
file is enough; say which language you speak natively.

A new language is a new file with the same keys, plus its name in
`src/i18n/index.ts`, `src-tauri/src/i18n.rs` and
`src-tauri/crates/core/src/config.rs`. Open an issue first, so that nobody
translates five hundred texts twice.

## Security rules for code

- Nothing Home Assistant sends is run as code; commands map to fixed actions.
- Anything that reveals what the user does, or lets Home Assistant act beyond
  the built-in buttons, starts switched off.
- Secrets live in Windows Credential Manager, never in `config.json`, logs or
  the repository.

The threat model is in [docs/SECURITY.md](docs/SECURITY.md).

By contributing you agree that your work is published under the
[MIT license](LICENSE) and that you follow the
[code of conduct](CODE_OF_CONDUCT.md).
