# Kabuk

A desktop shell for Windows built from scratch: a bar on every monitor, dynamic tiling and floating windows, a launcher, notifications, animated and created wallpapers, a screen saver and a settings app. OLED-friendly, Turkish and English, does not break games. The Windows taskbar is hidden and Explorer is left untouched; when Kabuk exits, Windows returns to its previous state.

Open source under the Apache License 2.0 (see `LICENSE`).

> Status: version 1.0.6. `docs/PLAN.md` has each milestone's results and what is left.

## Installing

Download `kabuk-setup.exe` from the [latest release](https://github.com/Milvasoft/kabuk/releases/latest) and run it. Windows 11 is needed; no administrator rights are asked for. The setup puts Kabuk in `%LOCALAPPDATA%\Programs\Kabuk`, adds "Kabuk" to the desktop and the Start menu (with "Kabuk Settings"), adds the `kabuk` command to your `Path`, asks whether Kabuk should start when you sign in, and starts it.

The setup takes `/S` to install without asking. To remove Kabuk, use "Uninstall" in Windows' "Installed apps", or `kabuk --uninstall`: Windows is put back as it was; your settings stay in `%APPDATA%\kabuk`.

Settings → About → "Check for updates" tells you when a newer version is out; its setup installs over the running one and keeps your settings.

## Building

Requirements: Windows 11, [Rust](https://rustup.rs) (`x86_64-pc-windows-msvc`), Visual Studio Build Tools (C++ workload + Windows SDK), the WebView2 Runtime (part of Windows 11). Node.js only for the design tools and the settings page test.

```powershell
cargo build --release --workspace
```

The four programs land in `target\release`: `kabuk.exe` (the shell), `kabuk-watchdog.exe` (starts the shell and puts everything back if it crashes), `kabuk-settings.exe` and `kabuk-saver.exe` (the screen saver and the animated wallpapers).

From the folder of the programs, `.\kabuk.exe --install` installs what you built, as the setup does. `powershell -ExecutionPolicy Bypass -File tools/package.ps1` builds the setup itself (`dist\kabuk-setup.exe`). `kabuk --startup on` or `off` changes starting at sign-in later.

## Using

- `kabuk-watchdog.exe` starts Kabuk; the Start menu's "Kabuk" does the same.
- `Win+R` opens the launcher; the shortcuts are in Settings → Shortcuts.
- `kabuk msg <command>` talks to the running shell (`kabuk --help` lists the commands).
- Settings live in `%APPDATA%\kabuk\kabuk.toml`; the settings app writes it and Kabuk applies every change live.

## Emergency

If anything is left hidden: `Ctrl+Alt+Shift+End`, or `kabuk.exe --restore`. Every hidden window, the taskbar, the work area, the cursors, the screen saver and the wallpaper come back. The watchdog does this by itself when the shell crashes.

## Checks

```powershell
powershell -ExecutionPolicy Bypass -File tools/check.ps1
```

Formatting, clippy with warnings as errors, every unit test, the Windows integration tests (they run the real shell on your desktop for a few seconds at a time; on CI they are left to the test VM, `tools/vm/run.ps1`) and the design page's checks. `docs/TESTING.md` has the rules.

## Documents

| File | Contents |
| --- | --- |
| `docs/CONTRIBUTING.md` | Working rules: safety, code style, what "done" means |
| `LICENSE` | License (Apache 2.0) |
| `docs/SPEC.md` | Product definition and acceptance criteria |
| `docs/ARCHITECTURE.md` | Technical design |
| `docs/PLAN.md` | Milestones, their results and "done" criteria |
| `docs/TESTING.md` | Test rules |
| `docs/DECISIONS.md` | Decisions |
| `docs/design/` | Live design page and handoff notes |
| `reference/OledSaver/` | The screen saver Kabuk's scenes were ported from (C#) |
| `tests/fixtures/contrast.json` | Expected results of the contrast engine |
| `tools/` | The check script, the design page's extractors and audits, the settings page test |
