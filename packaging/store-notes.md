# Notes for the Microsoft Store submission

What Partner Center asks for that is not the listing itself (DECISIONS D84). The listing's texts are in `locales/en.ftl` and `locales/tr.ftl` (`store-listing-*`).

## Why the package declares restricted capabilities

**runFullTrust.** Kabuk is a desktop shell written in Rust with the Win32 API: a bar on every monitor, a tiling window manager, a launcher and a settings app. It must run as a full-trust desktop program to arrange other programs' windows, show its own bar in place of the Windows taskbar and host the system tray.

**unvirtualizedResources** (HKCU and AppData write virtualization disabled; Partner Center takes at most 500 characters):

> Kabuk's looks set Windows' own cursors, screen saver, wallpaper, light/dark mode and accent colour (HKCU Control Panel\Cursors and Desktop, Themes\Personalize, Explorer\Accent, DWM). Windows reads them from the real HKCU, and the files they point to (cursors, screen saver, wallpapers Kabuk draws) from the real AppData; virtualized, they do nothing. Every value is recorded first and written back on exit, by the watchdog and by kabuk --restore (Ctrl+Alt+Shift+End).

## Notes for certification

- Kabuk replaces the Windows taskbar with its own bar while it runs, and arranges windows into tiles. This is its purpose. To leave it at any moment: **Ctrl+Alt+Shift+End**, or `kabuk --restore` in a terminal; every hidden window, the taskbar, the work area and every changed setting come back.
- Start Kabuk from Start ("Kabuk"); its settings are "Kabuk Settings", also in Start.
- It starts at sign-in through the package's startup task once it has been started, and can be turned off in Settings › Apps › Startup.
- No account, no sign-in and no purchase. The optional "Create" wallpaper feature asks for the tester's own Claude or OpenAI API key, or a local Ollama; everything else works without it.
- Source code: <https://github.com/Milvasoft/kabuk>. Privacy policy: <https://github.com/Milvasoft/kabuk/blob/main/PRIVACY.md>.
