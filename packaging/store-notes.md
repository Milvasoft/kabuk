# Notes for the Microsoft Store submission

What Partner Center asks for that is not the listing itself (DECISIONS D84). The listing's texts are in `locales/en.ftl` and `locales/tr.ftl` (`store-listing-*`).

## Why the package declares restricted capabilities

**runFullTrust.** Kabuk is a desktop shell written in Rust with the Win32 API: a bar on every monitor, a tiling window manager, a launcher and a settings app. It must run as a full-trust desktop program to arrange other programs' windows, show its own bar in place of the Windows taskbar and host the system tray.

**unvirtualizedResources** (HKCU and AppData write virtualization disabled). Kabuk's looks set Windows' own settings while it runs: the mouse cursors (`HKCU\Control Panel\Cursors`), the screen saver (`HKCU\Control Panel\Desktop`), the wallpaper, and the light/dark mode and accent colour (`HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`, `…\Explorer\Accent`, `HKCU\Software\Microsoft\Windows\DWM`). Windows reads these from the real registry and the files they point to (the cursor files, the screen saver and the wallpaper pictures Kabuk draws under `%LOCALAPPDATA%\kabuk` and `%TEMP%`) from the real AppData; virtualized, none of them would take effect. Every value is recorded before it is changed and written back when Kabuk exits, when its watchdog sees it end, and with `kabuk --restore` (also Ctrl+Alt+Shift+End). Each of these changes can be turned off in Kabuk's settings.

## Notes for certification

- Kabuk replaces the Windows taskbar with its own bar while it runs, and arranges windows into tiles. This is its purpose. To leave it at any moment: **Ctrl+Alt+Shift+End**, or `kabuk --restore` in a terminal; every hidden window, the taskbar, the work area and every changed setting come back.
- Start Kabuk from Start ("Kabuk"); its settings are "Kabuk Settings", also in Start.
- It starts at sign-in through the package's startup task once it has been started, and can be turned off in Settings › Apps › Startup.
- No account, no sign-in and no purchase. The optional "Create" wallpaper feature asks for the tester's own Claude or OpenAI API key, or a local Ollama; everything else works without it.
- Source code: <https://github.com/Milvasoft/kabuk>. Privacy policy: <https://github.com/Milvasoft/kabuk/blob/main/PRIVACY.md>.
