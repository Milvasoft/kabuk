# Kabuk: work plan

The order is deliberate: first the safety net, then the visible part (the bar), then the hard core (the window manager). At the end of every milestone there is a usable, tested build. No milestone starts before the previous one is finished.

Status markers: `[ ]` not started, `[~]` in progress, `[x]` done (all criteria tested). This file is updated when each milestone is finished.

Each milestone's tests follow the rules in `docs/TESTING.md`. The tests listed here are the minimum.

---

## M0 · Foundation and safety net `[x]`

- Cargo workspace and the crate skeleton from `ARCHITECTURE.md §3`; the `windows` crate version is pinned.
- Per-Monitor V2 DPI manifest, single-instance mutex, `tracing` logging.
- `kabuk-core`: `Clock` trait (real + fake), state file read/write (`state.json`, atomic write).
- `kabuk-watchdog`: starts `kabuk.exe`, waits, restores on abnormal exit, restarts (at most 3 in 60 s).
- `kabuk.exe --restore` and the `Ctrl+Alt+Shift+End` emergency exit.
- Base configuration: `kabuk.toml` schema (all settings from SPEC, with defaults), parsing, validation, error message.
- First version of `kabuk-testapp` (class name `KabukTestWindow`, behavior selected via the command line).
- `tools/check.ps1` and GitHub Actions (`windows-latest`).

**Done criteria**
- [x] `tools/check.ps1` clean.
- [x] A test window is written to the state file as "hidden" and hidden, `kabuk.exe` is force-killed → the watchdog shows it within 2 s (integration test).
- [x] `kabuk --restore` works in every state (integration test).
- [x] Configuration round-trip and invalid value tests pass.

**Result (2026-10-12)**
- Tests: 77 `kabuk-core`, 6 `kabuk-i18n`, 20 `kabuk-win` unit/property/snapshot tests; 14 Windows integration tests (`apps/kabuk/tests/safety_net.rs`). Line coverage: `kabuk-core` ≈ 96 %, `kabuk-i18n` 92 %, `kabuk-win` 77 % (its remaining lines are the desktop-changing paths covered by the integration tests).
- Tried by hand: `taskkill /F /IM kabuk.exe` with a hidden test window → shown again and the shell restarted; a real `Ctrl+Alt+Shift+End` key press → window shown within ~2 ms, watchdog and shell exited; `--debug-*` switches rejected by the normal build; CLI errors and usage printed in the terminal.
- Bug found and fixed: coordinate APIs returned scaled values to DPI-unaware callers, so a work-area record could be applied in the wrong unit and shrink the work area (happened once on the development machine during a test and was restored at once). Now all coordinates are physical (`kabuk_win::dpi::physical`), and a work-area record is only re-applied if its monitor still matches. Both have regression tests.
- Later (2026-10-15, found using Kabuk): the shell froze for a quarter of an hour (the window manager waited for a lock it held itself) and the watchdog did not notice, as it only waited for the process to end. The deadlock is fixed and a clippy lint keeps the pattern out; the watchdog now ends a shell whose bar or window manager stops answering for 15 s, restores and restarts it (DECISIONS D55). Test: `watchdog_restarts_a_shell_that_stopped_answering` (a shell made to hang with `--debug-hang-after-ms`; its hidden window back and a new shell answering in about 20 s), and the `kabuk_core::hang` unit tests.
- Not done here: the GitHub Actions workflow is written but has not run yet (the repository has no remote). The "state file unwritable" startup path shows a message box and has no automated test. Live config reload and config-error notifications come with M1; `kabuk msg` with M7. Taskbar hiding stays off until M4; registry backup and DWM border reset are added to the restore list together with the features that change them (M9, M3).

## M1 · Bar (Ada, dark mode) `[~]`

The taskbar **stays visible** at this stage.

- `kabuk-ui`: D3D11/D2D/DComp, DirectWrite, embedded fonts, icons, node tree and simple layout.
- A bar on every monitor; space reserved on all four edges with AppBar; DPI changes; monitor add/remove.
- Modules: workspace indicator (fake data at this stage), title, clock, date, media, CPU, RAM, GPU, network, volume. Module on/off setting.
- `kabuk-i18n`: tr/en, date/number formats; language setting.
- Live configuration reload (file watching).
- OLED: bar shifting.

**Done criteria**
- [x] The bar is on the correct edge on both monitors; a maximized window does not go under the bar (all four edges tried by hand, screenshots). *Automated on the real desktop for all four edges on both monitors (`tests/bar.rs`: work areas and a maximised window's frame). By hand on 2026-10-13: the running shell's `bar.position` was switched live to top, bottom, left and right; on each edge both work areas shrank by the bar (65 px tall or 85 px wide at 125 %), the maximised window on Monitor 1 (ana) ended at the bar, and the bottom bar sat above the Windows taskbar. Screenshots taken.*
- [ ] Text is sharp on a monitor with a different DPI. *Per-monitor DPI is implemented (each bar renders at its monitor's DPI, `WM_DPICHANGED` rebuilds); both monitors here are 125 %, so a mixed-DPI setup has not been tried. The user chose to try this at the end of the project.*
- [x] A screenshot was placed side by side with the Ada bar on the design page and the differences were fixed. *Kabuk's drawing code rendered off-screen (`examples/bar_shot`) next to `tools/design-shot.js`, compared with `tools/compare-shots.py`, top and left edges. Fixed: workspace numbers not centred, title spacing, text weight (gamma). Remaining difference: the browser uses ClearType, a transparent composition surface only grayscale.*
- [x] Turning a module off removes it from the bar immediately (tried by editing `kabuk.toml` by hand). *Test edits the file of a running shell; the next drawing lacks the module.*
- [x] Changing the language changes the date and the decimal separator. *Live: weekday names and `18,6G` ↔ `18.6G` switch without restart.*
- [x] 10 min idle CPU < 0.1% (performance test). *0.057 % (exact, cycle counters) / 0.062 % (Windows ticks), private memory 10.5 MB (`tests/perf.rs`, release).*
- [x] When Explorer restarts, the bar reserves its space again. *`TaskbarCreated` handling tested (bars rebuild and re-reserve). Real restart on 2026-10-13 (release build under the watchdog, both monitors at 125 %): Explorer was killed and came back by itself; the shell kept running (same PID), logged the shell change, rebuilt both bars at 2560×65, and both work areas stayed at y 65–1380; the screenshot shows the bar above the restored Windows taskbar.*

Status: all criteria but mixed DPI are met; the user chose to try mixed DPI at the end of the project.

**Result so far (2026-10-13)**
- Tests: 182 unit/property/snapshot, 20 Windows integration (14 safety net, 6 bar on the real desktop), 1 performance.
- Bugs found and fixed with a failing test first: bars rebuilding each other forever on `ABN_POSCHANGED`; 1 px borders blurred at 125 %; workspace numbers not centred.
- Measured and fixed for the idle budget: the hardware D3D device alone cost ~48 MB (DECISIONS D14: WARP rasteriser, composition stays on the GPU); `GetIfTable2` cost 0.9 ms per sample (interfaces now cached); a system-wide title hook woke Kabuk for every window's title change (now only the focused process); full-bar redraws for one changed number (now partial DirectComposition updates).
- Not in M1 by plan: tray, notifications and power modules (M4/M9), real workspaces (M3), light and timed modes and the other six looks (M2), config error notifications (M9).

## M2 · Theme engine `[x]`

- `tools/`: a script that extracts theme tokens (`themes/a…g.toml`), translations (`locales/*.ftl`) and icons from the design HTML. No manual copying.
- `kabuk-theme`: token model, light/dark, mode options (Dark, Light, Follow Windows, By time of day), accent color, contrast engine (SPEC §4), text size.
- Bar drawing for all seven looks (including the Rıhtım dock; at this stage the dock is a fixed app list).

**Done criteria**
- [x] All combinations in `tests/fixtures/contrast.json` match the native engine exactly. *112 cases (7 looks × 2 modes × 8 accents), byte-identical hex output (`kabuk-theme` contrast tests).*
- [x] Contrast audit: 7 looks × 2 modes × 8 accents, all text roles ≥ 4.5:1. *`resolve` audit test over every combination, plus the design page's own audit in `tools/check.ps1`.*
- [x] A screenshot of each look's dark and light bar was compared with the design. *Off-screen (`examples/bar_shot` vs `tools/design-shot.js`, both at 1×) for all 7 looks in both modes, plus Rıhtım's dock (`--dock`, 429×68 in both). On the real desktop (both monitors at 125 %) for all 14 combinations, each started in a sandbox and restored. Fixed from the comparisons: group borders did not take space like a CSS border-box (Devre 1–2 px off); 1.5 px borders were rounded up instead of down as Chromium does; Rıhtım's title lacked the 10 px gap before the app name; and on a real wallpaper the bars without their own background (Çizgi, Devre, Konsol) were transparent, so light-mode text was unreadable — they now sit on `--desk` as the design's `.has-wall` rule says (DECISIONS D16). Remaining differences: text set by DirectWrite is up to ~1.5 % wider than Chrome's for the same font (Ada's title capsule 3 px over 240); the equaliser is still (animation is M10); tray, notification and power modules are M4/M9, so the right end differs.*
- [x] With the mode set to "Follow Windows", Kabuk changes immediately when the Windows theme changes. *Integration test: Windows' mode comes from a test file (`--debug-app-mode-file`, so the user's theme is never touched), `WM_SETTINGCHANGE` "ImmersiveColorSet" is sent to the bars and the light look is applied before the send returns. A real switch in Windows Settings has not been tried yet; it only needs the user to flip the app mode once with Kabuk running.*

**Result (2026-10-13)**
- Theme engine: `kabuk-theme::resolve` (mode now from Dark / Light / Follow Windows / By time of day, flips at 07:00 and 19:00 by timer; accent override with the contrast engine adapting it; text size, DECISIONS D15). Live: mode, accent and text size apply from `kabuk.toml` without restart (integration test).
- All seven looks draw from one style table (`bar/style.rs`); Rıhtım's dock is its own AppBar window on Monitor 1 (ana), opposite the bar, with the design's fixed tile list (DECISIONS D17 is open on what it should hold and do).
- Renderer additions: gradient fills, per-span trailing space, icon stroke width, Chromium-like border rounding.
- Bug found by a new test and fixed: notifications (`WM_SETTINGCHANGE`, `TaskbarCreated`, display changes) that arrived while the bars were busy were dropped — a window procedure that runs inside an AppBar or COM call could not borrow the state, and a nested message to the same window never reached its handler. Shell surfaces now take nested messages, note these notifications without borrowing and retry from a timer.
- Tests now: 26 bar unit tests, 8 bar integration tests on the real desktop (dock, follow Windows and live appearance added), 14 safety-net tests; `tools/check.ps1` clean.
- Idle cost after M2 (`tests/perf.rs`, release, 600 s): CPU 0.046 % from cycle counters, 0.076 % from ticks; private memory 10.9 MB.

## M3 · Window manager core `[~]`

- `kabuk-layout`: dwindle and master, gaps, property tests.
- WM thread, events, manageable-window filter, invisible border correction, `SWP_ASYNCWINDOWPOS`.
- Per-monitor workspaces, `SW_HIDE` + state file, real data for the bar's workspace indicator.
- Shortcuts (SPEC §6): workspace, move, focus, layout change, close. Shortcut conflict check.
- Smart floating (dialogs, splash screens, owned windows, windows that don't fit), elevated (admin) windows.
- Active window border (DWM border color).
- Monitor changes and wake from sleep.

**Done criteria**
- [x] The layout property tests in TESTING §3 and the relevant integration tests in §4 pass. *`kabuk-layout`: all six layout properties (no overlap, inside the area, exact gaps, add/remove round trip, floating windows don't matter, unfit windows float and the rest stays valid) over random monitors, gaps and 0-20 windows, plus a random-operation property for the workspace model (the screen always shows exactly the active workspaces, visible tiled windows never overlap, through switches, moves, floats, refusals, minimise/restore and monitor unplug/plug); line coverage 96.6 %. Integration (`tests/wm.rs`, test-mode window manager, `kabuk-testapp` windows only): tiling on open and reflow on close; workspace switch hides into the state file and shows in place; forced kill → the watchdog shows hidden windows within 2 s and takes the accent frame off; `--restore` with windows on several workspaces exits 0; a minimum size that doesn't fit floats; a hung window does not slow actions; owned dialogs float and splash screens are ignored; a gap change in the file is applied within 500 ms; a clean exit takes the frame off; a window refusing to move floats; a self-resizing window does not disturb Kabuk; a minimised window comes back to its place; swap/float/close on the focused window; the bar shows the real workspaces; shortcuts are registered and a chord held by another program is reported; a cloaked window (another virtual desktop) leaves the layout; a failure inside the window manager ends the shell so the watchdog restores the desktop (a panic in a window procedure aborts the process; the panic is logged first).*
- [x] With a hung window present, shortcuts work within 100 ms. *Round trip of a workspace switch with a hung window present is a few ms; the other window is hidden and shown within 100 ms.*
- [ ] The apps on the compatibility list were tried by hand, `docs/compat.md` was filled in, nothing breaks. *Needs the user: the real window manager moves and hides every open window, so it has only run in test mode so far.*
- [ ] Monitor unplug/plug and sleep/wake tried by hand; no window was lost. *Implemented (display, work-area and resume notifications re-read the monitors; an unplugged monitor's windows go to the primary monitor and back) and covered by model tests; trying it by hand needs the user.*
- [x] Shortcut to layout < 16 ms (measured). *Worst of 5 layout changes with 3 windows: 2.8 ms in a debug build (logged as `apply_us`).*

**Result so far (2026-10-13)**
- Done: `kabuk-layout` (dwindle, master, gaps scaled per monitor DPI, unfit windows float; the workspace model with switch, move, float, swap, focus by direction, minimise/restore to the old place, monitor unplug/plug); the window filter (`kabuk_core::wm`: ignore, float, tile); the WM thread with out-of-context WinEvents, async placement with invisible-border correction, read-back of refused placements, state-file records before hiding and before colouring the frame, the accent frame, monitor/work-area/resume re-reads, the SPEC §6 shortcuts with a report for chords another program holds; the bar shows the real workspaces and switches them on click.
- Bugs caught by tests and fixed: a new window took the focus (windows opening in the background would steal it); cloaked windows stayed in the layout; a minimised window came back at the end instead of its old place; the watchdog binary in the tests was stale (the crash test caught it).
- Idle cost with bar and window manager (`tests/perf.rs`, release, 600 s): CPU 0.050 % from cycles, 0.065 % from ticks; private memory 10.8 MB.
- Waiting for the user: the real window manager on the real desktop (compatibility list, monitor unplug/plug, sleep/wake, elevated windows such as Task Manager, focus by shortcut, which Windows only grants to real key presses).

## M4 · System tray and hiding the taskbar `[~]`

- Spike first: the ARCHITECTURE §9 approach is validated on Windows 11 with a separate small program; the result is written to DECISIONS. *Done: `crates/kabuk-win/examples/tray_spike.rs`, result in DECISIONS D19.*
- Tray module: icons, tooltips, left/right click, icon updates, Explorer restart.
- Hiding and restoring the taskbar (with backup), added to the watchdog's restore list.

**Done criteria**
- [~] Discord, Steam, OneDrive, volume, network and Bluetooth icons are visible and their menus open next to the icon. *On the user's desktop the bar showed every notify icon Explorer had (Defender, OneDrive, Bluetooth, Twinkle Tray, NVIDIA Broadcast, Logitech G HUB, Chrome, espanso, WhatsApp). Discord and Steam were not running, so they were not seen. Volume, network and battery are not notify icons on Windows 11 (Explorer draws them itself, D19): the bar's own volume and network modules stand in for them. Clicks reach the program in the format of its icon version (integration test: right click → WM_RBUTTONUP + WM_CONTEXTMENU, double click → WM_LBUTTONDBLCLK, version-4 layout). That the menus open next to the icon still needs the user's eye.*
- [x] When Explorer restarts, the icons and the hidden taskbar recover by themselves. *Real restarts on 2026-10-13 with the release build under the watchdog: Kabuk saw Explorer's process end, gave its tray window up, waited for the new Explorer to announce its taskbar, then took the tray again, hid the taskbar and rebuilt the bars; work areas y 65–1440 on both monitors and all 9 icons back on the bar. Integration tests cover the recovery after the announcement and the 20 s fallback without one.*
- [x] On a forced kill, the taskbar comes back within 2 s (integration test). *`tests/tray.rs`: shown within 2 s by the watchdog, auto-hide put back.*

**Result so far (2026-10-13)**
- Working: a hidden topmost `Shell_TrayWnd` takes the programs' notify icons and forwards everything to Explorer (Explorer keeps every icon, so a crash loses nothing); icons drawn from their `HICON`s; left, right and double clicks with `AllowSetForegroundWindow`; tooltips (D21); icons of programs that quit dropped; `bar.modules.tray` live; the taskbar recorded, set to auto-hide and kept hidden by a WinEvent hook on Explorer's process; Explorer restarts recovered by watching its process.
- Found by trying it on the real desktop and fixed (each with a test where a test can show it): Explorer shows its taskbar again on its own; showing it puts its tray window in front of Kabuk's so programs register with Explorer; a new Explorer that finds Kabuk's tray window never announces its taskbar; AppBar calls forwarded through Kabuk's window are answered but do not reserve space (Kabuk's own AppBar calls now go to Explorer directly); asking programs to register again made Explorer show its taskbar, which asked again (now at most every 30 s); one `TaskbarCreated` broadcast reaching every bar was handled once per bar; the busy cursor over the bar; a test that picked the tray icon by position once clicked another program's icon (NVIDIA Broadcast's window opened and was closed again) and now picks it by its window.
- Idle cost as for the user (`tests/perf.rs`, release, 600 s, bars, tray, hidden taskbar and the window manager): CPU 0.066 % from cycles, 0.044 % from ticks; private memory 14.8 MB.
- Not done yet: other programs' own AppBars (third-party docks) while Kabuk holds the tray (their calls are forwarded and may not reserve space); icon contrast on light bars.

## M5 · Floating windows and rules `[~]`

- Float/tile shortcut; float on drag from the title bar (`EVENT_SYSTEM_MOVESIZESTART/END`); a click without movement does not float; double-click to re-tile.
- Alt+left drag to move, Alt+right drag to resize (`WH_MOUSE_LL`, Alt masking).
- Per-workspace "fully floating" layout.
- Window rules engine (exe, class, title; plain/regex) and default rules.

**Done criteria**
- [x] The drag-and-drop tests in TESTING §4 pass. *`tests/wm.rs`: a window dragged by its title floats where it is dropped and the other window takes the space; a click without movement changes nothing; with "Drag to float" off it goes back. The testapp reports the drag as Windows' move loop does (`EVENT_SYSTEM_MOVESIZESTART/END` around a move), without mouse input.*
- [ ] After Alt is released, the application menu does not get focus (tried with Notepad, Explorer, Office). *Needs the user: tests may not send input, and the hook ignores injected input anyway. Implemented as DECISIONS D23 (mask key when a grab starts).*
- [x] After adding a rule, the matching window follows the rule the next time it opens (integration test). *`tests/wm.rs`: a rule written to `kabuk.toml` while Kabuk runs sends the next matching window to workspace 2; other tests cover a workspace, float + placement at the right edge, and ignore.*

**Result so far (2026-10-13)**
- Working: the rules engine (`kabuk_core::rules`, DECISIONS D22) with the built-in rules, applied to new windows (float, ignore, workspace, monitor, minimum width, placement) and reloaded live; drag to float from the title bar, a window dropped on another monitor joins its active workspace; per-workspace floating layout from a click on the bar's layout indicator (which shows the active workspace's layout), the default layout applied live to workspaces not switched by hand; Alt + left drag to move and Alt + right drag to resize through a low-level mouse hook on its own thread, the mask key, double click on a floating window's title to tile it again (DECISIONS D23); the float/tile shortcut (from M3).
- Bugs caught by tests and fixed: a panic inside the window manager aborted the process instead of reaching its own failure handling (panics in Windows callbacks are now raised again in the message loop); a held window drifted back by one step because its invisible borders were measured while it moved.
- Tests: 385 in `tools/check.ps1`, among them 10 for the grab state machine, 9 new layout tests, 8 new Windows integration tests.
- Not done yet: the settings screen and translations for the window settings and rules (the settings app is M8; the settings exist in `kabuk.toml` and apply live); the "Floating layout" / "Tiling layout" notices of the design (notifications are M9); trying Alt + drag by hand; the Alt + click conflict in Blender, VS Code and Photoshop is an open question in D23.

## M6 · Launcher `[~]`

- `kabuk-search`: fuzzy matching, Turkish letters, ranking, calculator.
- Sources: Start menu shortcuts and `shell:AppsFolder` (including Store apps), recent files, indexed folders, browser bookmarks (Edge/Chrome), system commands, **open windows** (all workspaces).
- Layout according to the seven looks; keyboard; closes on click outside.

**Done criteria**
- [x] The search tests in TESTING §3 pass. *`kabuk-search`: Turkish folding ("EKRANI" → "Ekranı kilitle", "istanbul" → "İstanbul", "ılık" ≠ "ilik"), matched positions (property test), the calculator (`12*8`, `(3+4)/2`, `2^10`, `9,8+1`, invalid input), stable ranking (property test, Turkish alphabet order on equal scores).*
- [x] Shortcut to first frame < 100 ms (measured); search per keystroke < 5 ms with 5,000 items. *First frame 7.9 ms in a debug build with 949 entries (logged as `first_frame_us`; the integration test asserts < 100 ms); worst keystroke over 5,000 entries 1.3 ms (`kabuk-search/tests/perf.rs`, release).*
- [x] "EKRANI" → "Ekranı kilitle" tried in the real launcher. *On the user's desktop with the user's 949 entries (apps, recent files, Documents, Chrome/Edge bookmarks), opened by a test message and typed into its window: "Ekranı kilitle" first. The first try put the bookmark "E-Arşiv Portal Giriş Ekranı" first; a match at the start of a title now outranks one at a later word (regression test). The shortcut itself still needs the user: Alt+Space is taken on this machine by another program (most likely espanso's search bar).*

**Result so far (2026-10-13)**
- Working: `kabuk-search` (Turkish case folding and collation, best-placement fuzzy matching with highlighted letters, calculator); the launcher model and its drawing for the seven looks (compared with `tools/design-shot.js --launcher mil` for every look); placement per look and bar edge; entries from open windows (front first, choosing one shows its workspace), installed apps (`shell:AppsFolder`, desktop and Store apps), system commands, recent items, the indexed folders and Chrome/Edge bookmarks of every profile, gathered on a worker on first use and refreshed after five minutes (DECISIONS D24); Enter opens through the shell, reveals the window, runs the command or copies the result; Esc and a click elsewhere close it; the shortcut is live from `shortcuts.launcher`.
- Bugs caught by tests and fixed: Konsol's one-line rows were centred; the ranking above; a tray test raced the asynchronous taskbar hide.
- Not done yet: the settings screen and translations for the launcher page (M8; `[launcher]` and `shortcuts.launcher` apply live from `kabuk.toml`); real app icons (rows show the kind's icon, as the design does); the open animation (M10); trying the shortcut and a click outside by hand.

## M7 · IPC and command line `[x]`

- Named pipe server, `kabuk msg …` client, `query state|windows|monitors` JSON output.
- End-to-end scenario tests are written against this interface.

**Done criteria**
- [x] Every command in ARCHITECTURE §12 is verified by an integration test. *`tests/ipc.rs`, through `kabuk msg` against a shell with bars and a test-mode window manager: `workspace` and `move-to` hide and show a test window, `focus`, `float toggle`, `layout dwindle|master|floating|tiling`, `reload` reaching the bars and the window manager once each, the three queries' JSON, `launcher`, `restore` answering, stopping the shell and showing a window of another workspace; a typo explained without a shell, no shell (exit 15), a shell without the window manager (exit 16).*
- [x] The pipe is open only to the current user (ACL test). *The pipe's DACL holds exactly the user's SID (`kabuk-win` unit test and `tests/ipc.rs` on the running shell's pipe); a second server on the same name is refused.*

**Result (2026-10-13)**
- Working: `\\.\pipe\kabuk-<SID>-<instance>`, served on its own thread; `kabuk msg <command>` from any terminal (tried on the user's desktop: `kabuk msg query state` answered by the running shell).
- Bugs caught by tests and fixed: the pipe server waited forever on its next connection when stopped, and clients failed while it was between connections; the window manager read the configuration once per change notification (three times for one save).
- Found while the user watched a full-screen video during the run, fixed with the tests that caught them: the tray took a program's add while Explorer brought its own tray forward, so the program's next call went to Explorer (the tray is now put back in front after every forwarded call).
- Tests: 483 in `tools/check.ps1`.

## M7a · Bar apps and tray overflow (requested by the user on 2026-10-13) `[~]`

- `taskbar` module: pinned apps (Windows' own taskbar pins) and open apps' icons, grouped by program, with a running mark; a click opens or shows the app; on/off in settings (DECISIONS D25).
- Tray overflow: `bar.tray_shown` icons on the bar (default 3), the rest in a panel opened from an overflow button; the panel closes on a click elsewhere (DECISIONS D26).
- Resizing tiled windows by dragging the edge between them: the neighbours give or take the space, as with Windows' snapped windows (DECISIONS D27).
- A floating window that opens under the bar is moved into the work area (reported by the user: Snipping Tool). *Done: `kabuk-layout` test.*
- What lies behind the bar over the wallpaper: solid desk colour, Windows' blur, or nothing (requested by the user; DECISIONS D28). *Solid seen on the user's desktop; blur not yet seen.*
- Bar height setting (requested by the user; DECISIONS D29). *Done: integration test.*
- Apps icons sharper (loaded at twice their size, drawn on whole pixels), the web apps pinned from Chrome and Store apps with their own icons, windows that cannot be resized float (the League of Legends client was tiled and left white; all reported by the user). *Done: tests.*
- Rıhtım's dock holds the apps; its last tile opens the launcher (DECISIONS D17). *Done: integration test.*

**Done criteria**
- [x] The module and the overflow are drawn in every look and compared with the bar's own groups (no design exists for them). *`bar_shot --apps 5 --tray 3 --tray-more 4` for all seven looks: each sits in the look's own group style; Ray showed no apps on its vertical rail (fixed: they stack down the rail, test); Rıhtım's apps are in its dock (D17, decided by the user).*
- [x] Turning the module off and changing `tray_shown` apply live (integration test). *`the_apps_module_follows_its_setting_live`, `icons_beyond_tray_shown_are_clicked_from_the_overflow_panel`.*
- [~] A click on an app's icon shows its window (integration test); a pinned app that is not running starts (tried by hand). *Shown, and minimized when clicked while in front as on Windows (asked for by the user): `an_apps_icon_shows_its_window_and_minimizes_it_when_in_front`. Starting a pinned app by hand is left to the user.*
- [x] A hidden tray icon is clicked from the panel (integration test). *`icons_beyond_tray_shown_are_clicked_from_the_overflow_panel`.*
- [~] Dragging the edge between two tiled windows resizes both and the layout keeps the new share (integration test with the testapp's fake resize, tried by hand). *Integration test passes (`tests/wm.rs`); trying it by hand is left to the user.*

## M8 · Settings app `[~]`

- `kabuk-settings.exe` (WebView2). The UI is derived from the settings screen on the design page; 12 pages.
- Every setting end to end: UI → `kabuk.toml` (atomic write) → Kabuk applies it live → UI reads it back. Translations from the same `.ftl` files.
- Live info (monitor count, managed windows, RAM) via IPC.
- Mouse cursor set (SPEC §2.9 Appearance, DECISIONS D20): the user's cursors (`HKCU\Control Panel\Cursors`) recorded in the state file, the set applied with `SPI_SETCURSORS`, restored on exit, by `--restore` and by the watchdog.

**Done criteria**
- [ ] Every setting in SPEC §2.9 was changed one by one and its effect was seen in Kabuk (checked off as a list).
- [x] Dimming timeout "Custom" field: below 10 s, above 60 min and an empty value give an error; a valid value is saved. *`tools/settings-test.js` against the real bridge: 5 s, 61 min, empty and 2.5 min refused with the error shown, 2 min saved as 120 s, 45 s saved.*
- [x] With the settings window closed: zero processes, zero memory. *`kabuk_ayarlari_opens_one_settings_window`: closed normally, the app ends and no WebView2 helper with its profile is left.*

**Result so far (2026-10-13)**
- Working: the 12 pages of the design's settings screen, its markup and stylesheet taken from the design page (`tools/extract-settings.js`, checked in `tools/check.ps1`), every word from the language files; every choice, switch and chip writes `kabuk.toml` through the bridge (only that key changes, comments kept, invalid values refused and said so); shortcuts are recorded from the keyboard; window rules are added and removed; Windows' own dialogs pick a wallpaper image and indexed folders; live information comes from the running shell; the launcher's "Kabuk ayarları" opens one settings window. New settings of this milestone and M7a are on their pages: bar background, height, tray icons on the bar, the apps module, mouse cursors (D20, a set per look drawn by Kabuk, the user's recorded first and restored on exit, by `--restore` and by the watchdog), the Alt + click exemption in rules (D23).
- Tested: `tools/settings-test.js` drives every page in Chromium against the real Rust bridge; integration tests for the settings window, the cursor set and its restore.
- Not done yet: changing every setting by hand and seeing its effect in Kabuk (the first criterion); the wallpaper, screen saver and Create pages write their settings, but those features come with M9, M12 and M13; "Check for updates" has no update service yet (the button is off and says so).

## M8a · Other programs' windows follow the look (requested by the user on 2026-10-13) `[~]`

- `windows.follow_look` (DECISIONS D30): title bar colour and text colour, corners and frame of the managed windows from the look's tokens, through DWM only (no injection); back to Windows' defaults on exit, when turned off, by `--restore` and by the watchdog.
- Settings → Windows: the switch, wired end to end with its strings.

**Done criteria**
- [x] A test window's caption colour, text colour and corner preference follow the look and the mode, and return to the defaults when the setting is turned off and on `--restore` (integration test reading them back with `DwmGetWindowAttribute`). *`other_programs_frames_follow_the_look_and_come_back`: DWM reads back only the corner preference (the colours answer "invalid parameter", checked on this machine), so the corners are read back and the colours are checked through the calls DWM accepted and the state file's record; the look's colours per look and mode in a unit test.*
- [ ] Tried on the user's desktop with File Explorer, Notepad and a browser in Ada and Çizgi; screenshots compared with the design's windows.

## M9 · Game mode, OLED, notifications, screen saver `[~]`

- Fullscreen detection, game mode behaviors, notification queue and summary.
- Idle dimming (timer based on remaining time, media/fullscreen exception), early dimming in light mode.
- Kabuk notifications and OSD.
- Windows notification count: spike first (ARCHITECTURE §10), decision into DECISIONS.
- Screen saver selection: installing `reference/OledSaver`, writing and backing up the Windows settings, preview.
- Power menu.

**Done criteria**
- [~] Game mode within 1 s with a fullscreen test window (integration test); tried by hand with a real game (including one with anti-cheat). *`a_full_screen_window_turns_game_mode_on_and_the_layout_comes_back`: `query state` shows it within 1 s, the mouse hook is removed, a new window keeps its place, the tiled ones stay, the layout is the same afterwards and one summary notice comes. The user has played League of Legends (Vanguard anti-cheat) with Kabuk running all day; the new build (hook removed in game mode) still needs a game by hand.*
- [~] No dimming while media is playing; with no activity it happens after the configured time (test with fake clock + by hand). *Fake clock: `kabuk_core::oled` (the time left, held off while busy, light mode sooner); overlays and lifting on input: `dimming_covers_every_monitor_and_input_lifts_it`. By hand: left to the user.*
- [x] The screen saver selection is reflected in the Windows settings and reverts when Kabuk is uninstalled. *`the_screen_saver_choice_is_set_in_windows_and_comes_back`: OledSaver installed in the state folder, `SCRNSAVE.EXE`, the wait and the theme set; recorded first and written back on exit and by `--restore` (CONTRIBUTING.md rule 5, so also when Kabuk is removed).*

**Result so far (2026-10-13)**
- Working: game mode per monitor (bar, tiling, notices, the mouse hook; each switch of `[game]` honoured); Kabuk's notices in every look (layout switched, window floated or tiled again, result copied, game mode's summary) and the volume OSD; idle dimming (D31); the screen saver (OledSaver until Kabuk's own in M12, D12) with "Preview" in Settings and Kabuk starting it during video when asked; the bell with Windows' notification count (D32) opening the notification centre; the power button and its menu.
- Not done yet: the OSD and the notices were compared with the design only through their parts (no design screenshot of them yet); trying game mode, dimming and the saver by hand.

## M10 · Animation `[~]`

- Own surfaces: DirectComposition animations (launcher, notification, OSD, workspace indicator, mode transition).
- Window transitions: spike of the DWM thumbnail approach (ARCHITECTURE §7); if successful, full implementation, otherwise instant transitions and a decision record.
- Animation setting Full/Reduced/Off, follows the Windows setting, off in game mode.

**Done criteria**
- [x] No dropped frames on own surfaces (measured with DComp frame statistics). *Measured with the desktop compositor's own counters (`DwmGetCompositionTimingInfo`: frames composed, missed, dropped; `kabuk_win::screen::Frames`), which cover every surface DComp hands to DWM: no frame missed or dropped while a notice enters and leaves (`a_notice_fades_in_and_out_letting_the_mouse_through`), while the bars blend into a new mode (`a_mode_change_blends_the_bar_into_its_new_look`) and while other programs' windows move (`a_layout_change_plays_the_move_and_the_overlay_goes`). Monitor 1 (main) runs at 360 Hz: one frame per refresh in the thumbnail spike (D35).*
- [x] Input is not blocked while an animation runs; an interrupted animation settles into the correct end state. *Animations run on the compositor (own surfaces) or on the window-motion thread (other programs' windows), never on the bar or window-manager thread. A leaving popup lets the mouse through (screen and style read in the notice test); the window-move overlay is click-through and the real windows are already in their places. An interrupted move starts the next one from where it is (`kabuk_ui::motion::Motion`, unit-tested; the window mover takes the thumbnails' current places as the new start).*
- [x] The idle CPU limit is still met. *Measured on 2026-10-14 with the machine left alone (the user out, asked for): `cargo test --release -p kabuk --features perf --test perf` over 600 s with bars, tray, hidden taskbar and the window manager: CPU 0.082 % from cycles (489 ms), 0.081 % from ticks, private memory 32.2 MB (limits 0.1 % and 40 MB). Earlier runs had the user at the machine and the mouse hook alone took 0.31 %; the bell's count was moved to change notifications (D32).*

**Result so far (2026-10-13)**
- Working: `kabuk_ui::motion` (cubic ease-out, poses of opacity, scale and slide; an interrupted move starts from where it is; Full / Reduced / Off, off with Windows' animation effects or in game mode) and `WindowSurface::play` / `crossfade` on the compositor. Notices slide in 8 px over 220 ms and fade out letting the mouse through; the OSD slides up and is redrawn in place; the launcher, tooltips, the tray panel and the power menu grow in from scale .985 over 180 ms (the ones holding the keyboard leave at once, DECISIONS D33); a change of look or mode, and another workspace coming forward, blend the bars over 220 ms. Other programs' windows move with DWM thumbnails over a click-through overlay that shows the desktop under them (a thumbnail of Explorer's desktop window since M12, D35). `appearance.animations` is in Settings → Appearance and applies live.
- Tests: motion unit tests; notice entrance and exit (screen read, click-through, frames), animations off, the power menu's entrance, the bars' mode blend and workspace blend, window moves (overlay, wallpaper kept, frames, end places), the settings page writing `animations`.
- Found by the full checks and fixed: the dimming came back at once after posted input on a machine left alone; the bell's count broke the idle CPU budget; DWM gives no thumbnail of the wallpaper (the gaps went black during a move).
- Private memory 24.9 MB at start (limit 40 MB); the apps module's icons account for 11.5 MB of it (the shell's icon machinery loaded into the process). Moving icon extraction out of the process is a candidate if the budget gets tight.
- Not done yet: the idle CPU run on an untouched desktop; trying the transitions by hand.

## M11 · Hyprland equivalents `[~]`

- Hidden workspace (scratchpad), tabbed window groups, an overview showing all workspaces (with DWM thumbnails).

**Result so far (2026-10-13)** (how they work: DECISIONS D34)
- Hidden workspace: `Alt+Shift+S` sends the focused window there (hidden, recorded in the state file), `Alt+S` shows it over the focused monitor's workspace and hides it again; `kabuk msg scratchpad toggle|send`. Model in `kabuk_layout::Desk` (unit and random-operation property tests: a window minimised there comes back there, its monitor unplugged hides it).
- Tabbed groups: `Alt+G` makes the focused window a group or breaks it up, a window opening while the group has the focus joins it, `Alt+Shift+G` and a click on the strip switch tabs; the strip is Kabuk's own window in the look's colours. Two bugs found by the property test and covered by regression tests (a group split by a monitor coming back; a hidden tab "refusing" its size).
- Overview: `Alt+O` and `kabuk msg overview`; a cell per workspace on every monitor, live thumbnails on the shown one, boxes with icon and title on the others (Windows gives no thumbnails of hidden windows; the user's choice); a click goes there.
- Every shortcut is in `[shortcuts]`, in Settings → Shortcuts and in both translations, and applies live.
- Tests: `the_hidden_workspace_takes_a_window_and_shows_it_over_the_workspace`, `scratchpad_send_and_toggle_hide_and_show_the_window`, `a_tabbed_group_shows_one_member_under_its_strip`, `the_overview_shows_every_workspace_and_a_click_goes_there`.
- Not done yet: trying them by hand with real programs; tab titles follow a program's title change only when the window manager's view next changes.

## M12 · Desktop background `[~]`

- Spike first: the `WorkerW` approach is validated with a separate small program on both of Windows 11's window structures (before/after 24H2).
- Static backgrounds (`IDesktopWallpaper`), backup and restore.
- Shader runner, 30 fps cap, pause conditions; port of the Circuit eye and Constellation scenes.
- Background page (monitor selection, brightness, pausing), bar backdrop rule.

**Result so far (2026-10-13)**
- Spike (`kabuk-win/examples/workerw_spike.rs`, DECISIONS D13): on this machine's 24H2 structure a layered child of `Progman` behind the icons shows (95 % of Monitor 1 (main) took its colour). The pre-24H2 structure is coded but could not be tried: this machine runs a later build (26300) and no older Windows 11 is at hand.
- Still wallpapers: Windows' own (the default, untouched), pure black, the night gradient from the look's accent and desk colours, the user's picture; set per monitor through `IDesktopWallpaper`, the user's own recorded in the state file first and given back on exit, by `kabuk --restore` and by the watchdog. A running slideshow is left alone.
- Animated wallpapers (DECISIONS D36): the circuit eye and the constellation are pure Rust simulations (`kabuk-scenes`, OledSaver's constants) drawn with Direct2D in `kabuk-saver.exe --wallpaper`, a process of its own behind the icons, at most 30 fps; Windows' wallpaper under it turns pure black. It ends with Kabuk, stops drawing while windows cover its monitor or on battery (looked at once a second), and stops in game mode. The same program is the screen saver (`kabuk-saver.scr`).
- Window moves (D35) show a live thumbnail of the desktop under the moving windows, so an animated wallpaper keeps playing there instead of going black.
- Bars over a wallpaper keep the desk-coloured strip (SPEC §2.8); bar text is measured against it.
- CPU while an animated wallpaper is seen (`kabuk-ui/examples/scene_cost.rs`, per monitor, of one core at 30 fps): circuit eye 1.7 % at 1080p, 2.0 % at 1440p, 2.4 % at 4K; constellation 1.2 %, 1.8 %, 5.2 % (its links are drawn with flat ends, as OledSaver did, which took the 4K figure down from 14 %). Measured on the desktop with two 1440p monitors: about 4 % of one core for the circuit eye.
- Tests: `the_scenes_draw_a_4k_frame_in_under_4_ms_on_the_gpu`, `the_scene_canvas_draws_shapes_text_and_the_wordmark`, the scenes' unit tests, `an_animated_wallpaper_runs_while_chosen_and_ends_with_kabuk`, `the_animated_wallpaper_stops_in_game_mode`, the watchdog's wallpaper test after a forced kill, `a_scene_shows_the_desktop_under_its_windows`.
- Found by the full checks and fixed: while the machine saves power, Windows refuses the screen saver's settings; the restore then lost the user's settings (the state file forgot them anyway). Now the backups not given back stay recorded and the shell tries again each minute. `tools/check.ps1` passed except `the_screen_saver_choice_is_set_in_windows_and_comes_back`, which cannot pass while the machine is in that state (left alone for a while); it is to be run again with the user at the computer.
- Not done: the pre-24H2 structure (above); SPEC §2.8's slow drift of a still picture (OLED) is not built, because Windows shows the still wallpapers and Kabuk would have to write a shifted picture again and again; the user chose it on 2026-10-14 (DECISIONS D47) and it is built: the user's own picture is made once per picture and monitor size in four places of a two-pixel square (JPEG files in the state folder) and Windows is given the next one every `wallpaper.drift_minutes` (10), without the user's own between (`a_still_picture_drifts_a_step_at_a_time`; a switch on the OLED page).

**Done criteria**
- [x] With the desktop covered and on battery, GPU/CPU usage drops to zero (measured). Covered: the player's threads used 0 ms after the pause (0.07 % over 20 s, start-up included). Battery takes the same path (`power::on_battery`) but cannot be measured on this desktop computer.
- [x] Frame time < 4 ms at 4K (GPU timestamps): 83 µs for the circuit eye, 23 µs for the constellation.
- [x] When Kabuk exits, the previous wallpaper comes back; on a forced kill the watchdog brings it back.
- [x] With the background on, the contrast audit is clean for bar text: the text sits on the desk strip, which the audit checks.

## M13 · Create `[~]`

- `kabuk-wallgen`: providers, key storage, format, validation pipeline, three-attempt fix loop.
- Create tab.

**Result so far (2026-10-13)** (the terms: DECISIONS D37)
- `kabuk-wallgen` (pure, unit-tested): the format (`wall.toml`, and `wall.hlsl` as one `float3 wall(float2 px)` function Kabuk wraps), the system prompt with the rules and a sample, reading the model's reply, the requests and answers of Claude, OpenAI and Ollama, the OLED checks, the three-try loop and the store of created wallpapers in the state folder.
- `kabuk-win`: Credential Manager and WinHTTP wrappers (tested against Credential Manager and a local server). `kabuk-ui`: compiling with `D3DCompile` and the bounded-work rule, timing 120 frames at 4K with timestamps, reading a minute of frames back.
- Settings → Wallpaper → Create: provider, API key (password field, Credential Manager) or Ollama's model, the request, the four steps as they happen with the try number, the preview, Apply (kept and chosen as the wallpaper) and Try again; a failure says why in the user's language. Created wallpapers are listed under Choose and play behind the icons like the built-in scenes.
- Tests: `created_wallpapers_are_judged_for_the_right_reason` (GPU), the store, format, prompt, reply, providers and pipeline unit tests, the settings page test (a wallpaper created end to end with a stand-in Ollama whose first answer is too bright and goes back, then applied; a key in no file or log; checked to fail when the key is logged), `a_created_wallpaper_plays_behind_the_icons`.
- Not done: criterion 2 needs the user's keys (and Ollama, which is not installed on this machine); the OpenAI model name `gpt-5` is an assumption to confirm with a real key.

**Done criteria**
- [x] Validation pipeline unit tests: sample shaders that don't compile, are slow, are too bright or have a fixed bright region are rejected for the right reason. (On the GPU: a syntax error, 10.6 ms at 4K, 30 % white bands, a still bright disc and a grey background are each refused for their reason; the sample passes at 0.05 ms.)
- [ ] A real background was generated with three providers (with the user's keys, by hand).
- [x] The key does not appear in plain text in any file or log (test).

## M14 · Release `[~]`

- Install and uninstall (into the user directory, without requiring admin), run at startup (with consent), update check.
- The full compatibility list, performance measurements, README.

**Result so far (2026-10-13)** (how: DECISIONS D38)
- `kabuk --install` (no administrator rights): the programs in `%LOCALAPPDATA%\Programs\Kabuk`, "Kabuk" and "Kabuk Settings" in the Start menu, an entry in Windows' "Installed apps"; it asks before starting at sign-in, and `kabuk --startup on|off` changes that. `kabuk --uninstall` undoes everything as `--restore` does, removes the stored API keys, the shortcuts, the entry, the sign-in value and the program folder (once it has ended), and keeps the settings. `kabuk --help` prints the usage.
- `tools/release.ps1`: every check, the release build and a zip of the four programs with the README and the licence (0.1.0: 8.6 MB) with its SHA-256.
- README rewritten for building, installing, using and checking.
- Tests: `kabuk_installs_for_the_user_and_uninstalls_without_a_trace` and `a_wrong_startup_value_changes_nothing` (in a temporary folder and a registry key of their own), the install and command-line unit tests.
- Found by the full checks and fixed: the shortcut test registered M11's new chords (Alt+S, Alt+G…) with their real defaults and counted too few.
- Not done: the update check waits for the user (DECISIONS D39, open: where the latest version is published); the compatibility list (`docs/compat.md`) and the idle CPU run need the user at the computer; the release has not been installed on the user's own account.

**Result (2026-10-14)**
- `kabuk-setup.exe` for end users (`tools/package.ps1` → `dist\kabuk-setup.exe`, ~23 MB, with its SHA-256): one file holding the four programs; it asks before installing, stops a running Kabuk, installs with `kabuk --install` (now also a desktop shortcut and the program folder on the user's `Path`, so `kabuk` works in any new terminal; both taken out again by `--uninstall`), asks about starting at sign-in and starts Kabuk; `/S` installs silently. The C runtime is linked in (`.cargo/config.toml`), so a fresh Windows needs nothing else.
- The update check (D39): Settings → About asks GitHub for the latest release and offers its page (unit tests; the page test runs it against GitHub through the real bridge).
- Rıhtım is the default look (D2).
- Tried: the setup installed silently on the user's account (programs, shortcuts, `Path`, uninstall entry, `kabuk --help` from `Path`, Kabuk running at 23 MB) and was then taken out completely. The clean Hyper-V machine (`tools/vm/try-setup.ps1`) was not signed in, so the fresh-Windows run waits for it.
- Not done: the compatibility list and the idle CPU run (need the user); the GitHub release itself (published as v1.0.0 on 2026-10-15; winget was dropped by the user on 2026-10-16).

## M15 · The user's requests after daily use (2026-10-14) `[~]`

Reported or asked for by the user on 2026-10-14, in this order of work (bugs first):

- Bug: on Monitor 2, Windows' own hidden taskbar slides in when the mouse reaches the bottom edge (Explorer had made that monitor's taskbar anew; the guard only knew the ones found at the start).
- Bug: the running shell wrote no log lines since it started.
- Bug: a window cannot be put where the user wants it, e.g. aligned to the left: dragging it there or snapping it puts it back. Dragging a tiled window and dropping it on a place in the layout or at a screen edge moves it there; Win + arrows move it (D40); a floating window snaps as on Windows.
- Bug: a click on the volume icon shows no volume slider. The bar's system icons (volume, network, battery, clock, notifications) do on a left and a right click what Windows does on its taskbar.
- Shortcuts on the Win key (D40), taken before Windows and other programs, so Kabuk's never clash with programs' Alt shortcuts or another program's hotkey (Alt+Space was taken in 13 of 30 starts).
- The apps on the bar work as on Windows' taskbar: dragging to reorder, a right click with the program's own tasks and recent items, a new window, pin and unpin, close; window previews on hover; a slight growth on hover.
- The power menu of every look gets "Kabuk Settings".
- Kabuk's logo (`assets/logo`) as the programs' icon, in Settings and wherever Kabuk shows its mark.
- Mouse cursors chosen apart from the look: style, colour and size, each previewed (D41).
- Minimising and restoring other programs' windows play macOS's genie: the window pours into its icon on the bar and back out.

**Result so far (2026-10-14)**
- Monitor 2's hidden taskbar: the guard knows every taskbar by its class and hides the ones Explorer makes anew (test). The "no log lines" report was the file system showing an open log's size late; the lines were there.
- Placing a window: a dragged window always floats where it is dropped, and one Windows snapped floats where it was snapped (decided by the user: "Hep yüzsün"; tests).
- The bar's system icons do on a left and a right click what Windows' taskbar does: the volume slider (drag on the track, the wheel), the clock's and the notification centre's menus, Task Manager and network settings (integration test).
- Shortcuts on the Win key with a low-level hook (DECISIONS D40); Settings' recorder takes Win chords with a hook of its own while it waits (page test; the real keys only by hand, tests never inject input).
- The apps on the bar (DECISIONS D45): dragged into another order, a right-click menu with recent files, a new window, pin or unpin and close, Kabuk's own pins in `pins.json`, a slight growth on hover, window previews with live thumbnails on hover (integration tests for the menu, pins, the order, the previews; unit tests for the growth and the drag).
- "Kabuk Settings" first in every look's power menu; Kabuk's logo as the programs' icon and in Settings.
- Cursors apart from the look (D41): five styles, three colours, three sizes (Windows' own pointer size, recorded and restored), each card previewed (integration test with the real cursors for a few seconds, page test).
- The genie (D46): Windows' own transitions off for managed windows (recorded and restored), the window poured into its icon on the bar and back out as 72 strips of its thumbnail; `windows.minimize_effect` (integration test, no frame missed; frames looked at mid-way).
- Found by the full integration run and fixed: a maximised window filling a monitor where nothing was reserved yet (Kabuk starting, Windows' taskbar hidden) was taken for a full-screen program, so that monitor's bar stepped aside and game mode came on (unit test first).
- The screen saver integration test fails while the displays are asleep (Windows answers 0x80070149, "an operation is in progress with the device"; Kabuk retries each minute); it passes with the displays on.
- Not done: tried by hand with the user (every item); custom jump-list tasks (e.g. WhatsApp's chats, Windows keeps them in an undocumented file); recent files only for programs with an app id of their own; the large cursor sizes were checked through Windows' reported pointer size, not by eye.

**Later requests (2026-10-14, evening)**
- An app's right click is its whole Windows jump list (D52): pinned entries, the app's own categories (Chrome's "Most visited", "Recently closed"), frequent or recent files, its tasks ("New window", "New Incognito window"), then its name, pin and close, each entry with its own icon (a site's favicon, a task's program, a file's or folder's icon) and the app's own picture on its name. Programs that set no app id are found by the id Windows makes up from their path (checked against Notepad's and PowerShell's lists); a program pinned from its window keeps its id, and one pinned by its path finds it in the Start menu's list ("Google Chrome", not "chrome"). Unit tests for the formats, the order, the ids and the pictures; tried on the user's desktop with Chrome's and File Explorer's menus (screenshots looked at: the same entries, names and icons as Windows' taskbar).
- Bug: a web app's pin (Jitsi Meet) opened a new window at every click: windows now go to pins by app id (D51; a failing test first).
- Bug: two volume indicators: the volume keys are Kabuk's while it runs (D50; unit tests for the keys).
- `Win+Shift+L` starts the screen saver now (the displays turn off when none is chosen), after the keys are let go; `kabuk msg saver` does the same; in Settings → Shortcuts (tr, en), live.
- Bug: the wallpaper stayed black after Kabuk ended: a run had recorded Kabuk's own black picture as the user's. A picture of Kabuk's is never recorded as the user's; the newest picture of Windows' wallpaper history that is not Kabuk's is recorded instead (unit test; tried on the desktop: Kabuk started over its own black picture and gave the user's back).
- Checked (2026-10-15, night): fmt, clippy, every unit test, the settings page test, the design checks; the integration run passes but for the screen saver test, which Windows refuses at night with 0x80070149 (it passed in a later run). Idle, with the user away: the shell 27 MB and 47 ms of CPU in 2 minutes (0.04 % of one core); the animated wallpaper 30 fps at 2.9 % of one core on two monitors, paused while covered.
- Not done yet: the volume keys and `Win+Shift+L` tried by the user's hand (tests never press keys).

**Using it on the user's desktop (2026-10-15)**
- Launcher: a word typed whole beats letters scattered over a long title ("chro" found bookmarks before Google Chrome); apps show their own icons (loaded on a worker); an empty launcher lists open windows, recent runs, then the bar's pins.
- Overview: a layout bug hid everything inside a layer's rows (the window boxes' icons and titles never showed); the overview holds the bar's workspaces, numbered, the shown one framed on every monitor.
- Bar: a window's preview has a × that closes it; a middle or Shift click starts another window; a click on an app with several windows goes to the next; no preview pops up after a click; a minimised window Windows keeps in front is brought back, not minimised again; every monitor's bar lists every app (D53); the volume panel chooses the output device (D54).
- Volume keys: the laptop's Fn keys come injected by keyboard software; they are taken now (tried by the user: one indicator).
- A shell that lost the desk leaves by itself (a test's shell had fought the user's over the tray all night).
- The genie: the window was minimised a compositor frame before its picture was on screen, so for one frame what is behind showed (found with `flash_probe`, which records every composed frame: 5 of 6 minimises); fixed, 0 of the following.
- The genie, again (found by the user: the desktop flashed as a window came back from the dock): the scene that follows the genie's first, once Windows minimises or restores the window, let the old thumbnails go before putting up its own, so a frame could show the bare desktop (caught with `flash_probe` clicking the dock icon); now the new ones go up first. Test: `a_following_scene_lets_the_old_thumbnails_go_last`; 0 flashes in 6 following minimises and restores.
- The shell froze for a quarter of an hour (a lock taken twice, shown by an elevated window); fixed, and the watchdog now restarts a shell that stops answering (DECISIONS D55, M0).
- Settings: the mode's hint says what the chosen mode does.
- The app grid (D56, asked for by the user): `appgrid::{model, view, backdrop}` unit tests (which apps, the order, the pages, the search, the arrows across pages, the strip, a drag's share, the wheel, the blur, the cells, the chrome); a wide picture stretched (render test); the button on every bar; Settings → Bar and Shortcuts (page test). Tried on the user's desktop: 196 apps on 6 pages of 7 × 5 in about 100 ms, the user turned pages; found and fixed then: the search field's icon and words at its top (test first), a black background over the black wallpaper (now the screen, blurred), the blurred picture dropped by the tray's refresh, tiles until the icons came (the first pages now load ahead).
- The gallery (D58, asked for by the user): 42 designs (two Milvasoft desktop wallpapers, ten live and ten still wallpapers, ten live and ten still screen savers), each checked with `gallery_check` at 4K (0.1-2.7 ms a frame; the savers OLED-safe) and looked at; played, drawn once or wandering as their category says; Settings → Wallpaper and Screen saver list them with previews. Tests: the table (every design in the format, the counts, the keys), the configuration, the still saver's wander and breath, the player (a still design drawn once and moved), the previews (drawn once, kept, old ones dropped), the names in both languages, the page (choosing, previews coming), and on the desktop `the_gallery_plays_live_designs_and_draws_still_ones` (a live design on both monitors, a still one as each monitor's picture, the user's own back).
- Found on the way: the drift's JPEG pictures were not known as Kabuk's (only BMPs were), so one could have been recorded as the user's own; fixed with the gallery's (test).
- The bar's colour (D57, asked for by the user): Settings → Bar, swatches and any colour, a note on hard-to-read text; tests for the schema, the palette, every look's strip and the page; tried on the user's desktop (Etkinlik, solid: the strip took the colour at once and gave it back).
- Bug (found by the user): on a bar at the left edge the clock sat over the apps in nearly every look; the centre moves aside and apps without room go behind a "+n" that opens the app grid (test first, every look).
- Bug (found by the user): in the overview the shown workspace's frame was covered at its top by a maximised window's thumbnail, which reaches past the work area; it is cut to its cell (test first; screenshot looked at).
- Bug (found by the user watching a video): Chrome's full screen keeps its window maximised at exactly the monitor's size, and the bar stayed over the video; such a window counts as full screen now (test first; tried: the bar stepped aside for the video).
- Three more looks from the design page, Pano, Izgara and Defter (D67, asked for by the user; not Piksel): tokens and 160 contrast cases from the page, the bar, launcher, notice and OSD styles, italic and letter-spaced text, capitals in the user's language, made-up small capitals, Roman numerals, Pano's task list, double and coloured rules, a `·` between groups, window corners per look, the Settings cards. Tests for each look's parts; `bar_shot` and `launcher_shot` compared with `tools/design-shot.js` for all three (bar, launcher, notice, OSD).
- Touchpad gestures (D68, asked for by the user): raw input from a precision touchpad, frames and swipes in `kabuk_core::gesture` (unit tests), a step to the neighbouring workspace (unit test), `[gestures]` in the schema, example and Settings › Windows (tr, en, live, page test), the bar's handling (integration test with a sent swipe). Not tried on a touchpad yet: neither this machine nor the VM has one (the user will try it).
- Checked: fmt, clippy, every unit test, the settings page and design checks, the integration run but `focus_toward_brings_the_neighbour_to_the_front`, which failed once while the user was using the machine (Windows refuses a foreground change then) and passed again alone.

**Done criteria**
- [ ] Each bug is first caught by a failing test, then fixed ✔; tried on the user's desktop (waits for the user).
- [ ] Every item works end to end (settings, both languages, live), with its tests ✔, and was tried by hand with the user (waits for the user).

## M16 · System theme (requested by the user on 2026-10-14) `[~]`

Windows itself follows the look, layer by layer, each one on and off on its own (SPEC §2.10, DECISIONS D42, ARCHITECTURE §11c). Starts after M15. In this order, the safest layers first:

- Design first: the "System theme" group of Settings → Appearance and each look's icon set are drawn on the design page and shown to the user before the code. Drawn on 2026-10-14 (`systemIcon` in the design page: 15 icons per look from the look's tokens, so they follow mode and accent; the group with its switches); **waits for the user's look** before any code.
- `[system_theme]` in the configuration schema, the restore records in the state file, the switch in Settings with its strings (tr and en), live reload. *Done: `colors` in the schema; the colour switch on Settings → Appearance (tr, en), live. (The `icons` and `explorer*` keys and switches were added and then removed with their layers, below.)*
- Colours: Windows' mode and accent from the look. *Done (2026-10-14): `system_theme.colors`, the switch in Settings → Appearance (tr, en), live; `windows_colours_follow_the_look_and_come_back` writes and reads back the real values for a few seconds and checks them back exactly, live and after `--restore`. Not yet looked at by eye with the user.*
- Icons: **built, then removed** (2026-10-14, by the user's decision, after the Explorer layer). It was built as per-look `.ico` sets written to the registry (file kinds through `Software\Classes\<ProgID>\DefaultIcon`, This PC and the Recycle Bin through their `CLSID\…\DefaultIcon`), and the user-found key bug was fixed (This PC and the Recycle Bin also written to the per-user `…\Explorer\CLSID\{clsid}\DefaultIcon` the desktop reads). But tried by hand on Windows 11 26300, **no override showed**: the per-user `Explorer\CLSID` override is ignored for the desktop and navigation-pane icons, and the machine-wide folder/drive override (`HKLM\…\Shell Icons`, tested with a UAC prompt) is ignored too — even after clearing the icon cache and restarting Explorer. The only working folder-icon mechanism on this build is the per-folder `desktop.ini` (what Windows' own "Customize" and tools like FolderIco use), which cannot theme every folder; the one global way would be patching `imageres.dll`, a risky, update-fragile system-file change Kabuk will not make. Store-associated file types (`.png`, `.jpg`, `.mp4`, `.pdf`) take their icon from the app package, so their per-user override never wins either. With no safe way to make it visible, the whole icon layer (`kabuk_core::system_icons`, `kabuk_win::system_icons`, `kabuk_ui::system_icons`, the host, `system_theme.icons`, the Settings switch and preview, the state record and `--restore` path, `tools/extract-system-icons.js`) was removed. The colour layer stays.
- Explorer layer: **built, then removed** (2026-10-14, by the user's decision). It was built as `kabuk-explorer.dll` loaded into `explorer.exe` only, theming File Explorer through the reversible DWM caption/immersive-dark attributes and uxtheme app-mode (no memory patching, no inline hooks). Tried by hand on the development machine (Windows 11 26300), it produced **no visible result**: Windows 11's File Explorer draws its own tabbed title bar and its content in XAML, so `DWMWA_CAPTION_COLOR` has no effect on the title bar and immersive dark is already set by the colour layer; forcing `SetWindowTheme` on the frame instead broke the XAML layout. There is **no safe, documented way** to theme Win11 Explorer to the look's colours — only the risky XAML/inline-hook path (Windhawk-style), which breaks on updates and carries anti-cheat risk, and the user chose not to take it. The whole layer (the `kabuk-explorer` crate, `kabuk_win::inject`/`explorer_theme`/`explorer_ipc`, `kabuk_core::system_theme`, the host, the config keys, the Settings switches, the state record and the `--restore` path) was removed; the colour layer stays (and the icon layer, later also removed — below). Explorer keeps Windows' own look (which follows dark/light through the colour layer).
- Sounds: none (DECISIONS D44, decided by the user: Windows' own sounds stay).

The Explorer and icon layers were removed (see above), so their criteria no longer apply. **Only the colour layer remains**, and M16 is now that one layer:

**Done criteria**
- [x] The colour layer turns on and off live from Settings and from `kabuk.toml`, in every look and mode. *`windows_colours_follow_the_look_and_come_back`; the look→colour mapping per look and mode is unit-tested.*
- [x] Turned off, after exit, after Kabuk is killed (watchdog) and after `kabuk --restore`, every recorded value is exactly as before, absent values included. *Colours as before (`windows_colours_follow_the_look_and_come_back`, on the real registry for a few seconds).*

---

## M17 · Microsoft Store (requested by the user on 2026-10-17) `[~]`

Kabuk as a free MSIX on the Microsoft Store (DECISIONS D84), beside the GitHub setup. Submitted by 2027-01-17 (the name's reservation ends then).

- Packaged or not: `kabuk_win::package::is_packaged`. Packaged, start at sign-in through the package's `StartupTask` (`kabuk --startup on|off` too), `--install`/`--uninstall` refused with a message (tr, en), no GitHub update offer in Settings › About (the Store updates it; tr, en).
- `tools/msix.ps1`: the release build, the four programs, `packaging/AppxManifest.xml` (identity from Partner Center, version from the workspace, `runFullTrust`, `unvirtualizedResources` with HKCU and AppData write virtualization off, the startup task, the `kabuk.exe` app execution alias, capture and notification capabilities) and the tile and logo images made from the app icon; `makeappx pack` into `dist\kabuk-<version>.msix`.
- Tried in the test VM: the package installed (test certificate, VM only), Kabuk started from Start and at sign-in, cursors, screen saver, wallpaper and colours reach Windows and come back, `kabuk --restore` through the alias, the package removed cleanly.
- The Store listing: privacy policy page, description tr/en, screenshots, age rating; submitted only after the user confirms.

**Done criteria**
- [ ] `tools/check.ps1` clean; the packaged/unpackaged switches unit-tested.
- [ ] The package installs, runs and uninstalls in the VM, every system value back after `--restore` and after removal.
- [ ] Partner Center accepts the package (no validation error) and the submission passes certification.

---

## Realistic expectations

The whole plan will not be finished in one session. A good first night: M0 and M1, maybe the start of M2. The window manager (M3) and the tray (M4) are the two hardest milestones; if rushed, they are where the bugs that will burn the user the most come from. The criteria are not loosened for speed.
