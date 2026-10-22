# Notes for the Microsoft Store submission

What Partner Center asks for that is not the listing itself (DECISIONS D84). The listing's texts are in `locales/en.ftl` and `locales/tr.ftl` (`store-listing-*`).

## Why the package declares restricted capabilities

**runFullTrust.** Kabuk is a desktop shell written in Rust with the Win32 API: a bar on every monitor, a tiling window manager, a launcher and a settings app. It must run as a full-trust desktop program to arrange other programs' windows, show its own bar in place of the Windows taskbar and host the system tray.

**unvirtualizedResources** was asked for at first and refused at certification on 2026-10-22 (policy 10.6.3). The package no longer declares it (DECISIONS D89): its HKCU and AppData writes stay the package's own, so the Store's Kabuk does not offer the look's cursors, its screen saver or Windows' colours (Settings says they are in the GitHub version), and writes its wallpapers to the package's own temporary folder, where Explorer reads them.

## Notes for certification

- Kabuk replaces the Windows taskbar with its own bar while it runs, and arranges windows into tiles. This is its purpose. To leave it at any moment: **Ctrl+Alt+Shift+End**, or `kabuk --restore` in a terminal; every hidden window, the taskbar, the work area and every changed setting come back.
- Start Kabuk from Start ("Kabuk"); its settings are "Kabuk Settings", also in Start.
- It starts at sign-in through the package's startup task once it has been started, and can be turned off in Settings › Apps › Startup.
- No account, no sign-in and no purchase. The optional "Create" wallpaper feature asks for the tester's own Claude or OpenAI API key, or a local Ollama; everything else works without it.
- Source code: <https://github.com/Milvasoft/kabuk>. Privacy policy: <https://github.com/Milvasoft/kabuk/blob/main/PRIVACY.md>.
