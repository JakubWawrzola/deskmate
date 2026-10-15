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
| `src-tauri/` | The Windows app: Rust (Tauri 2) |
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
`aarch64-pc-windows-msvc` target. Before a pull request:

- `npx tsc --noEmit` and `cargo check` without errors or warnings;
- `cargo test` in `src-tauri` for logic you touched;
- for the integration: it loads in Home Assistant, and the HACS and hassfest
  checks of this repository stay green;
- user-facing text in plain English, without jargon;
- docs updated when behaviour changes (`docs/GUIDE.md`, `CHANGELOG.md`).

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org)
(`feat:`, `fix:`, `docs:`, ...). Keep one topic per pull request.

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
