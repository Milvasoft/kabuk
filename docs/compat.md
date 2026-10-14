# Compatibility list

Problematic applications tried by hand with the real window manager (ARCHITECTURE §13, PLAN M3). Each row is filled in after trying, on the user's machine, with the build named in the row.

What is tried for each application:

1. **Tile:** it tiles when it opens and the gaps are even (no extra margin from invisible borders).
2. **Workspace:** switching away hides it, switching back shows it in the same place, it does not appear on the wrong workspace by itself.
3. **Dialogs:** its dialogs and pop-ups float and follow it (Save As, Settings, pickers).
4. **Minimise and maximise:** minimised it leaves the layout, restored it comes back; maximised it stays maximised.
5. **Close and restore:** closing it reflows the rest; `kabuk --restore` and a forced kill bring it back if it was hidden.

| Application | Build | Tile | Workspace | Dialogs | Min/max | Close/restore | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Discord | — | | | | | | |
| Spotify | — | | | | | | |
| Steam | — | | | | | | |
| Visual Studio | — | | | | | | |
| Rider | — | | | | | | |
| VS Code | — | | | | | | |
| Windows Terminal | — | | | | | | |
| Edge | — | | | | | | |
| Chrome | — | | | | | | |
| Word | — | | | | | | |
| Excel | — | | | | | | |
| Outlook | — | | | | | | |
| Task Manager (elevated) | — | | | | | | Expected to float: it runs elevated. |
| Settings (UWP) | — | | | | | | |
| File Explorer | — | | | | | | |
| OBS | — | | | | | | |

Not tried yet: the window manager has only run in test mode (it manages `kabuk-testapp` windows and nothing else). Trying it on the real desktop needs the user present, since it moves and hides every open window.
