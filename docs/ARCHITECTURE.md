# Kabuk: technical design

## 1. Principles

1. **Near-zero cost when idle.** No continuously spinning loop. Everything is event-driven; timers are set for the next real change (the clock to the minute boundary, idle dimming to the remaining time).
2. **Never touch another process.** No DLL injection, no global (in-context) hooks. Only documented Win32/WinRT APIs. This is the foundation of both anti-cheat compatibility and stability. The single exception, chosen by the user (DECISIONS D42): the opt-in Explorer theme layer loads `kabuk-explorer.dll` into `explorer.exe` and nowhere else (§11c).
3. **Never hang because of another process.** A hung window cannot make Kabuk wait: windows are moved with `SWP_ASYNCWINDOWPOS`, messages are sent with `SendMessageTimeout`.
4. **Always reversible.** Everything Kabuk changes in Windows (hidden windows, taskbar, work area, registry) is saved first and written back on exit, or by the watchdog process after a crash.
5. **Platform-independent core.** Layout, configuration, search, contrast and translation are pure Rust and can be tested on Linux too. Win32 only at the edges.

## 2. Processes

| Process | Role | Lifetime |
| --- | --- | --- |
| `kabuk-watchdog.exe` | Starts at sign-in, launches `kabuk.exe` and waits on its process handle, asking the bar's and the window manager's windows every 5 s whether they answer; a shell silent for 15 s is ended (DECISIONS D55). On abnormal exit it restores from the state file (shows windows, brings back the taskbar, resets the work area), then restarts Kabuk (at most 3 times in 60 s; beyond that it stops and notifies the user). | Always; very small (a few hundred KB, ~1 MB RAM) |
| `kabuk.exe` | Bar, window manager, launcher, notifications, OSD, idle dimming, IPC server. The `kabuk.exe msg …` subcommand is the IPC client (writes output via `AttachConsole(ATTACH_PARENT_PROCESS)`). `kabuk.exe --restore` is the emergency restore. | Always; one per desk: a shell or watchdog starting makes the one there give way (the emergency stop and restore of `--restore`), whatever its state directory, then takes over (asked for by the user on 2026-10-14; `kabuk-win::takeover`, the desk's owner in a session-wide named block) |
| `kabuk-explorer.dll` | The Explorer theme layer (§11c), loaded into `explorer.exe` by `kabuk.exe` only while `system_theme.explorer` is on. | While Kabuk runs; unloads itself when `kabuk.exe` ends |
| `kabuk-settings.exe` | Settings app. Runs the settings screen from the design inside WebView2; writes `kabuk.toml`, uses IPC for live information. | Only while open (see DECISIONS D3) |

Run at startup: only the watchdog, under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Not added without user consent.

## 3. Repository layout (Cargo workspace)

```
kabuk/
  Cargo.toml                 workspace
  crates/
    kabuk-core/              config schema and loading, state model, IPC message types, event types
    kabuk-layout/            pure layout engine: dwindle, master, floating, monitor/workspace tree
    kabuk-theme/             theme tokens, light/dark, contrast engine (fitContrast, fillPair)
    kabuk-i18n/              Fluent-based translation, date/number formats, Turkish case mapping
    kabuk-search/            launcher fuzzy matching and ranking, calculator
    kabuk-wallgen/           Create: provider clients, wallpaper format, validation pipeline
    kabuk-win/               safe Win32/WinRT wrappers (window, monitor, hook, appbar, DWM, audio, media, stats)
    kabuk-ui/                Direct2D/DirectWrite/DirectComposition rendering, small node tree, layout, animation, icons, fonts
  apps/
    kabuk/                   main application (bar, wm, launcher, notifications, ipc)
    kabuk-watchdog/
    kabuk-settings/          WebView2 shell + web/ (settings UI, derived from the design HTML)
    kabuk-explorer/          cdylib loaded into explorer.exe for the Explorer theme layer (§11c)
    kabuk-testapp/           test app that mimics problematic window behaviors (see §13)
  assets/
    fonts/                   OFL fonts (Geist, Geist Mono, IBM Plex Sans Condensed, IBM Plex Mono, Chakra Petch, JetBrains Mono, Manrope, Martian Mono, Figtree, Cantarell) and their licenses
    icons/                   SVG paths extracted from the <symbol> icons in the design
  themes/                    a.toml … g.toml (each with [dark] and [light] token tables + metrics)
  locales/                   tr.ftl, en.ftl
  tools/                     scripts that extract themes/translations/icons from the design HTML, contrast audit
  docs/
```

`kabuk-layout`, `kabuk-theme`, `kabuk-i18n`, `kabuk-search` and `kabuk-core` contain no Windows dependencies.

## 4. Threads

- **UI thread** (`kabuk.exe` main): message loop and rendering for all Kabuk windows (bar, launcher, notification, OSD, dimming overlay).
- **WM thread**: its own message loop; `SetWinEventHook(WINEVENT_OUTOFCONTEXT)` callbacks arrive here. The window state model lives here; layout is computed and applied here. Sends events to the UI over a channel (`workspace changed`, `title changed` …).
- **Input thread**: `WH_MOUSE_LL` (Alt+drag) and, if needed, `WH_KEYBOARD_LL`. Low-level hooks must return within 300 ms; this thread never waits on anything, it pushes events onto the channel. Hooks are removed in game mode.
- **Background pool**: launcher indexing, stats sampling, file watching.

Shared state is moved by messages, not locks (crossbeam channels or `PostThreadMessage`).

## 5. Bar

- One top-level window per monitor: `WS_POPUP`, `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP`; invisible in the task switcher, does not steal focus.
- Space reservation: `SHAppBarMessage(ABM_NEW / ABM_QUERYPOS / ABM_SETPOS)`, edge `ABE_TOP/BOTTOM/LEFT/RIGHT`. `ABN_POSCHANGED` and `ABN_FULLSCREENAPP` are handled.
- Taskbar: `Shell_TrayWnd` and every `Shell_SecondaryTrayWnd` are hidden, auto-hide state is set with `ABM_SETSTATE` and the work area is released. The previous state is backed up. If Explorer shows the taskbar again (`EVENT_OBJECT_SHOW`), it is hidden again. **The taskbar is not hidden until the system tray has moved into the bar (PLAN M4).**
- Explorer restart: when the `RegisterWindowMessage("TaskbarCreated")` broadcast is received, the taskbar is hidden again, the appbar is re-registered and the tray is set up again.
- DPI: Per-Monitor V2 awareness (manifest). The bar is rescaled on `WM_DPICHANGED`; every metric is theme value × DPI.
- Draw only on change: modules mark themselves "dirty", and on the next frame the UI redraws only that region.
- OLED shift: the content is shifted (drawing offset), not the window, by 1-2 px every few minutes.

### Modules and data sources

| Module | Source | Update |
| --- | --- | --- |
| Clock / date | `GetLocalTime`, i18n format | Single timer aligned to the minute boundary |
| Title | `EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_NAMECHANGE` (focused window only) | Event |
| CPU | `GetSystemTimes` delta | 2 s (while the bar is visible; stops during dimming and in games) |
| RAM | `GlobalMemoryStatusEx` | 2 s |
| GPU temperature | `D3DKMTQueryAdapterInfo(KMTQAITYPE_ADAPTERPERFDATA)` → `Temperature` (WDDM 2.4+; otherwise the module is hidden) | 5 s |
| Network | `GetIfTable2` byte delta (connected, non-loopback interfaces) | 2 s |
| Audio | Core Audio `IAudioEndpointVolume` + `IAudioEndpointVolumeCallback`, default device change via `IMMNotificationClient` | Event |
| Media | WinRT `GlobalSystemMediaTransportControlsSessionManager` | Event |
| Tray | §9 | Event |
| Notifications | §10 | Event |
| Power | `LockWorkStation`, `SetSuspendState`, `ExitWindowsEx` | Click |

### Module interface

Every module implements the same trait; WASM plugins will later use the same contract:

```rust
trait BarModule {
    fn id(&self) -> &'static str;
    /// Which events wake this module (clock tick, foreground change, audio, timer interval, ...).
    fn subscriptions(&self) -> Subscriptions;
    fn update(&mut self, event: &Event) -> Dirty;
    /// Returns a small layout description; kabuk-ui draws it with the theme and contrast rules.
    fn view(&self, ctx: &ViewCtx) -> Node;
    fn on_input(&mut self, input: &ModuleInput) -> Option<Action>;
}
```

`Node` is a small tree (Row, Column, Text, Icon, Meter, Badge, Spacer). A module does not pick colors, it picks roles (`fg`, `dim`, `accent`, `badge` …); colors come from the theme and the contrast engine.

## 6. Rendering (kabuk-ui)

- Direct3D 11 device → DXGI → Direct2D 1.1 device context; `CreateSwapChainForComposition` + a DirectComposition visual. Premultiplied alpha; rounded corners and transparent surfaces are drawn by us.
- Text via DirectWrite; fonts are loaded from `assets/fonts` with `IDWriteFactory5::CreateInMemoryFontFileLoader` (not installed system-wide).
- Icons: SVG path data extracted from the 23 `<symbol>`s in the design, embedded in a form that is converted to `ID2D1PathGeometry` at build time (24×24, 1.75 px stroke).
- On Windows 11, `DWMWA_SYSTEMBACKDROP_TYPE` (acrylic) for Rıhtım's glass surface; if unsupported, a plain semi-transparent surface.
- Our own small node tree and a simple flex layout (row/column, spacing, alignment). No heavy GUI framework (see DECISIONS D4).

## 7. Animation

- Our own surfaces (bar, launcher, notification, OSD): DirectComposition animations (`IDCompositionAnimation`) run on the compositor; they put no load on the CPU. Durations 120-220 ms, ease-out curve, no bounce; opacity, position and scale only.
- Application windows: animating another process's windows with `SetWindowPos` every frame stutters with heavy applications. Approach: when the layout changes, an opaque black click-through overlay is opened over the monitor; the **DWM thumbnails** (`DwmRegisterThumbnail`, live content) of that monitor's windows are animated from the old position to the new one while the real windows are placed at their final positions in one go; the overlay is removed after 150-200 ms. Under the windows' thumbnails the overlay shows a live thumbnail of Explorer's `Progman` (the wallpaper, an animated wallpaper and the icons as they are; DECISIONS D35). This approach is first validated with a spike in M10; if it turns out inconsistent, window transitions stay instant and our own surfaces still animate.
- Minimising and restoring (the genie, DECISIONS D46): Windows' own transition is turned off for each managed window (`DWMWA_TRANSITIONS_FORCEDISABLED`, recorded in the state file and turned back on by every restore path), so the window minimises or comes back at once; the same overlay then covers the whole monitor and draws the window's thumbnail as 72 horizontal strips, each a `rcSource` band of it, bending into the app's icon on the bar or back out (`kabuk_core::genie`, pure). A minimised window's thumbnail keeps its last picture (M15 spike). The bar tells the window manager where each window's icon is whenever that changes.
- The animated wallpaper pauses while it cannot be seen: covered, on battery, and while Windows has the displays off (`GUID_CONSOLE_DISPLAY_STATE` through a power setting callback, no window).
- Setting: Full / Reduced (opacity only) / Off. Off if `SPI_GETCLIENTAREAANIMATION` is disabled. Off in game mode.

## 8. Window manager

- **Events:** `EVENT_OBJECT_SHOW/HIDE/DESTROY`, `EVENT_OBJECT_CLOAKED/UNCLOAKED`, `EVENT_SYSTEM_FOREGROUND`, `EVENT_SYSTEM_MINIMIZESTART/END`, `EVENT_SYSTEM_MOVESIZESTART/END`, `EVENT_OBJECT_LOCATIONCHANGE` (filtered to tracked windows only), `EVENT_OBJECT_NAMECHANGE`.
- **Manageable window:** visible, not cloaked, not `WS_EX_TOOLWINDOW`, has no owner or its owner is invisible, has a title or `WS_EX_APPWINDOW`, its class is not on the blocklist (such as `Progman`, `WorkerW`, `Shell_*`, `Windows.UI.Core.CoreWindow`). For UWP, the relationship between `ApplicationFrameWindow` and the inner `CoreWindow` is resolved.
- **Invisible borders:** the shadow margin of Windows 10/11 windows is measured with `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` and the target rectangle is corrected for it; otherwise uneven gaps appear between windows.
- **Applying:** `SetWindowPos(SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS | SWP_NOOWNERZORDER)`. Then the actual size is read back; if the window did not fit into the requested size (minimum size), that window is marked floating and the user is notified ("Float windows that don't fit").
- **Maximized/minimized:** a minimized window leaves the layout and returns to its old place when restored. A maximized tiled window counts as a "fullscreen layout".
- **Workspace switching:** windows of inactive workspaces are hidden with `ShowWindow(SW_HIDE)`; the HWND is written to the state file before hiding (see the CONTRIBUTING.md safety rule). They do not appear in Alt+Tab; instead, the "open windows" source in the launcher lists windows across all workspaces. (Alternatives in DECISIONS D5.)
- **Focus:** when a shortcut reaches Kabuk, Kabuk counts as the process that received the last input, so `SetForegroundWindow` works. Methods that attach to another process's input queue, such as `AttachThreadInput`, are not used. A failed focus attempt is logged and caught by a test.
- **Active border:** on Windows 11, `DWMWA_BORDER_COLOR` gives the focused window the accent color and the others the default; on exit all of them return to `DWMWA_COLOR_DEFAULT`. On Windows 10, our own thin border window.
- **Layout engine (`kabuk-layout`):** input: monitor work area, gaps, window list (order, tiled/floating, minimum size hints), layout type and ratios. Output: a target rectangle for each window. A pure function, with property tests: no overlaps, no overflow outside the area, equal gaps.
- **Placement check:** 250 ms after placing, every window's frame is read back. A window larger than its place (minimum size) or away from it (it ignores moves) is placed once more; refusing twice floats it, so a slow window is not floated for answering late. Smaller is accepted (grid-snapped terminals).
- **Minimised windows** leave the layout with their place remembered and return to it when restored (to the monitor's active workspace if theirs is not shown).
- **Shared state file:** the bar (work areas) and the window manager (hidden windows, coloured frames) both write `state.json`, through one `Arc<Mutex<StateFile>>`; every entry is written before the change it describes. Only a change of the records that outlast Windows stopping (the user's settings, the taskbar, the work areas) waits for the disk (`FlushFileBuffers`), and those records also go to a flushed copy, `state.lasting.json`; the window records and the shell's pid are written without waiting (every program reads them at once, and they survive Kabuk ending in any way), since a flush took 20 to 90 ms in the test VM and a workspace switch must stay well under 100 ms. When Windows stopped before it wrote `state.json` out (a power cut can leave it filled with zeros), the restore moves it aside and undoes the copy instead; the windows the lost records were about went with Windows.
- **Test mode** (`--manage-class KabukTestWindow`): the window manager manages only `kabuk-testapp` windows and takes actions from a test message instead of global shortcuts, so integration tests never touch the user's windows or keys. `--no-wm` turns it off entirely (safety-net and bar tests).
- **Monitor changes:** `WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE(SPI_SETWORKAREA)`, `WM_POWERBROADCAST(PBT_APMRESUMEAUTOMATIC)` → re-enumerate with a 500 ms debounce. Monitors are identified by the device path from `QueryDisplayConfig`; the workspaces of a disconnected monitor move to the primary monitor, and when the monitor comes back the windows return to their old places.

## 9. System tray (high risk, early spike)

To receive tray icons while Explorer is running, we create our own window of class `Shell_TrayWnd` and keep it topmost; applications send their `Shell_NotifyIcon` calls (`WM_COPYDATA`) to this window, and we also forward every message to Explorer's real tray window. Then `TaskbarCreated` is broadcast so that all applications re-register their icons. Clicks are forwarded to the icon owner via `uCallbackMessage` (with NOTIFYICON_VERSION_4 semantics: `NIN_SELECT`, `WM_CONTEXTMENU`, coordinates in wParam); before that, `AllowSetForegroundWindow(owner pid)` is called so the menu can take focus.

A working open-source reference for this approach: **ManagedShell** (C#, MIT; the foundation of Cairo Desktop and RetroBar), the `TrayService` and `NotificationArea` classes. Native code is not copied from it; its behavior is used as a reference. PLAN M4 starts with a spike; if it breaks with Windows 11 updates, the fallback plan: the tray module falls back to a button that opens Explorer's overflow window.

## 10. Notifications

- Kabuk's own notifications and OSD: surfaces we draw ourselves.
- The Windows notification count requires `UserNotificationListener` (WinRT); this API requires package identity. Options: gain identity with a sparse package (signed), or reduce the module to an "Open notification center" button. Spike first in M9; the decision is recorded in DECISIONS.

## 11. Game mode and idle dimming

- **Fullscreen detection:** on `EVENT_SYSTEM_FOREGROUND` and on location changes of the focused window: does the window's extended frame fully cover the monitor rectangle, and is it not a desktop/shell window; additionally `SHQueryUserNotificationState` (`QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_PRESENTATION_MODE`, `QUNS_BUSY`). Event-driven; 1 s target.
- In game mode: that monitor's bar is hidden (appbar space is released), the WM does not tile new windows on that monitor, low-level hooks are removed, animation is turned off, notifications are queued.
- **Idle dimming:** the remaining time is computed with `GetLastInputInfo` and a single timer is set (no polling). Dimming: a black, semi-transparent click-through overlay (`WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE`) on each monitor; the bar lowers its own brightness. No dimming while media is playing (media session `Playing`) or a fullscreen application is present. The overlay is removed on the first input.

## 11b. Desktop wallpaper

- **Static wallpapers** (pure black, gradient, image): the image is rendered to disk and set per monitor via the documented `IDesktopWallpaper` COM interface. The previous wallpaper is backed up and written back on exit. For slow drift, the image is re-set shifted by 1-2 px every few hours.
- **Animated wallpapers:** to draw behind the desktop icons, message `0x052C` is sent to Explorer's `Progman` window to create a `WorkerW` behind the icons, and our own DirectComposition window is placed inside it. This message is undocumented (the method used by Wallpaper Engine, Lively and similar tools); it involves no code injection. The window structure changed in Windows 11 24H2; both structures are supported. On failure, animated wallpapers are disabled and fall back to their static counterpart (see DECISIONS D13).
- Rendering: one HLSL pixel shader per scene, a fullscreen triangle with Direct3D 11; 30 fps cap (`DCompositionWaitForCompositorClock` or a DXGI waitable object). The built-in scenes (Circuit eye, Constellation) are ported from the `reference/OledSaver` scenes.
- Pausing: desktop visibility is computed from the window rectangles the WM knows (it pauses if the whole area is covered); additionally fullscreen, game mode, `GetSystemPowerStatus` (battery) and session lock (`WTSRegisterSessionNotification`).
- **Create:** provider client (`kabuk-wallgen` crate): Anthropic Messages API, OpenAI API, Ollama local HTTP, sent through WinHTTP (`kabuk-win::http`). The key is stored in Windows Credential Manager via `CredWriteW` (`kabuk-win::credentials`). The model is given the format definition, the token list and a sample wallpaper; the returned `wall.hlsl` is compiled with `D3DCompile`, 120 frames are rendered at 4K into an offscreen render target and the duration is measured with GPU timestamps, the frames are downscaled and the average brightness and bright regions that stay constant for 60 s are computed (`kabuk-ui::render::GpuJudge`). Error text is fed back to the model (at most three attempts). The shader runs sandboxed: only constant buffers as input, no file/network access, and no dynamic flow control with a bounded instruction count, so no frame can run into the GPU timeout (TDR) (DECISIONS D37). The settings app runs it on a thread of its own and sends the steps to the page. A created wallpaper plays in `kabuk-saver --wallpaper` like the built-in scenes: drawn on the GPU into a texture and copied to the window's surface each frame.

## 11c. System theme

The layers and their rules are in DECISIONS D42; this is how they are built.

- **Recording:** every layer goes through the existing state-file record (as the cursors, the screen saver and the wallpaper do): the old value, or its absence, is written to `state.json` before Kabuk writes its own, and the restore path (exit, switch off, `--restore`, watchdog) writes it back. The value lists (which keys, which ProgIDs, which icon indexes) are pure data in `kabuk-core` with unit tests.
- **Colours:** `kabuk-win::personalize` reads and writes the `Personalize`, `Accent` and `DWM` values and broadcasts `WM_SETTINGCHANGE` with `SendMessageTimeout` (principle 3). The accent palette Windows expects (`AccentPalette`, eight shades) is computed in `kabuk-theme` from the look's accent.
- **Icons:** `kabuk-ui` renders each look's icons from `assets/icons` into `.ico` files (16 to 256 px) in the state folder. Per-user values (`HKCU\Software\Classes\<ProgID>\DefaultIcon`, the per-user `CLSID` icons) are written by `kabuk.exe`; the machine-wide `Shell Icons` values by a short elevated run of `kabuk.exe --system-icons apply|restore` started with `ShellExecuteExW` ("runas"), which records before it writes like everything else. A `RegNotifyChangeKeyValue` wait on the touched keys (on a worker thread, no polling) sets Kabuk's icon again when a program writes its own back. Refresh: `SHChangeNotify(SHCNE_ASSOCCHANGED)`.
- **Sounds:** a scheme under `HKCU\AppEvents\Schemes` with the files in the state folder (D44 open).
- **Explorer layer:**
  - Loading: `kabuk-win::inject` opens the target with only the rights it needs, checks that its image is `%WINDIR%\explorer.exe` in the user's session (anything else is refused, tested), writes the DLL's path and starts `LoadLibraryW` with `CreateRemoteThread`, then closes every handle. Kabuk loads it after Explorer's windows exist and again after Explorer restarts (the same `TaskbarCreated` path the bar already uses).
  - Inside Explorer: the DLL checks the versions of `explorer.exe`, `uxtheme.dll` and `Windows.UI.Xaml.dll` against the tried list and hooks nothing otherwise. Win32 drawing: hooks on the theme functions Explorer imports (`DrawThemeBackground`, `DrawThemeText`, `GetThemeColor`, `GetSysColor` and their kin), choosing the look's colour for each part and state and calling the original for everything else. XAML (Windows 11's toolbar, address bar, tabs, Home): `InitializeXamlDiagnosticsEx` with a small visual tree watcher that sets brushes and resources on the elements as they appear. Every hook is wrapped in `catch_unwind`; a failure turns that hook off and calls the original.
  - Tokens: a named, read-only file mapping made by `kabuk.exe` holds the look's tokens and mode; a named event signals a change, and the DLL repaints Explorer's windows. The DLL reads nothing else.
  - Unloading: a thread in the DLL waits on Kabuk's process handle and on an "unload" event; on either it removes its hooks, repaints and leaves with `FreeLibraryAndExitThread`. `--restore` sets the event and, if the DLL is still loaded after the timeout, restarts Explorer.
  - Crash guard: the time of every load is recorded; two abnormal Explorer exits within a minute of a load turn the layer off and say so.
- **Game mode:** the DLL is unloaded while game mode is on (unless `system_theme.explorer_in_game` is set) and loaded again after.

## 12. Configuration, translation, IPC

- **`%APPDATA%\kabuk\kabuk.toml`**: a single file, versioned with a `schema = 1` field; read with serde; an unknown key produces a warning, not an error. On an invalid file the last valid configuration remains and the notification shows the line/column. The directory is watched with `ReadDirectoryChangesW`, 100 ms debounce. The settings app writes to a temporary file and replaces atomically with `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. The sample schema is derived from the settings list in `docs/SPEC.md`; every key corresponds to a setting in SPEC.
- **Translation:** Fluent (`fluent-bundle`); `locales/tr.ftl`, `locales/en.ftl`. The initial content is extracted by a script from `L.tr`/`L.en` in the design HTML. A missing key is caught by a build test (the key sets of the two files must be equal).
- **IPC:** named pipe `\\.\pipe\kabuk-<user SID>-<instance id>` (the instance id comes from the state directory, so a test's shell never answers for the user's), a DACL with one entry for the current user, the first instance required (a squatter makes the server fail) and remote clients rejected; one JSON per line. The request carries the command's words (`{"args":["workspace","3"]}`), so client and server share one parser (`kabuk_core::ipc`); the answer is `{"ok":true,"result":…}` or `{"ok":false,"error":"<translation key>"}`. Commands, one for every `[shortcuts]` action and for the bar's panels (SPEC §8.23; a unit test holds each shortcut to its command's action): `workspace <n>|next|prev`, `move-to <n>`, `focus <direction>`, `move <direction>`, `float toggle`, `close`, `scratchpad toggle|send`, `group toggle|next`, `layout <dwindle|master|floating|tiling|cycle>`, `dropdown`, `desktop`, `launcher`, `overview`, `apps`, `keys`, `saver`, `panel quick|network|calendar|volume` (on the bar of the monitor in use), `profile <name>`, `reload`, `query state|windows|monitors|tray`, `restore`. The server runs on its own thread and only queues work for the window manager and posts to the bars' control window; `restore` answers before it signals the emergency stop. `kabuk msg` exit codes: 0 done, 2 bad command, 15 no shell running, 16 refused.
- **Log:** `tracing` → `%LOCALAPPDATA%\kabuk\logs\`, rotated daily, kept for 7 days.

## 13. Test strategy

- **Unit/property tests:** layout, configuration, search (including Turkish letter examples: "EKRANI" → "Ekranı kilitle"), contrast (same inputs as the algorithm on the design page → same outputs), translation key parity, date/number formats.
- **Windows integration tests** (`cargo test -p kabuk --features integration`, Windows only): `kabuk-testapp` mimics problematic behaviors: a window that enforces a minimum size, a window that refuses to move, a splash screen, an owned dialog, a slow-responding (hung) window, a window that resizes itself, an elevated window (manual). Expected for each: the layout does not break, Kabuk does not hang.
- **Problematic application list** (manual, every release): Discord, Spotify, Steam, Visual Studio, Rider, VS Code, Windows Terminal, Edge/Chrome, Office (Word/Excel/Outlook), Task Manager, Settings (UWP), File Explorer, OBS. Results are written to `docs/compat.md`.
- **Crash test:** `taskkill /F /IM kabuk.exe` → within 2 s all windows are visible and the taskbar is back. Restart Explorer → the bar and tray come back on their own.
- **Visual comparison:** `kabuk-ui` can render a module or the whole bar to a WIC bitmap; it is placed side by side with a same-size image captured from the design page with Playwright (not pixel equality, but visual comparison + contrast measurement).
- **Contrast audit:** the native contrast engine is run for every theme × mode × accent combination, and every text role is measured (see SPEC §4). `tools/contrast-audit.js` (Playwright) is also available for the design page.
- **Performance:** 10 min idle, CPU via `GetProcessTimes`, RAM via `PROCESS_MEMORY_COUNTERS_EX.PrivateUsage`; the result goes to the About screen and the CI output.
- **CI:** GitHub Actions `windows-latest`: fmt, clippy `-D warnings`, test (excluding integration tests), release build, binary size report.

## 14. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| The tray method breaks with a Windows update | Tray icons are not visible | Early spike (M4), fallback: overflow window button |
| AppBar calls forwarded through Kabuk's tray window are answered but reserve no space (seen in M4) | Kabuk's bars lost their space; other programs' AppBars (third-party docks) may still | Kabuk's own AppBar calls run with Explorer's tray window in front; other programs' AppBars to be tried by hand, otherwise forward them while temporarily stepping back the same way |
| Notification count requires package identity | No badge count | Sparse package or reduce to a button (M9) |
| Window transition animation stutters | The feel suffers | DWM thumbnail approach spike; otherwise instant transitions |
| `SW_HIDE` confuses some applications | A window gets lost in a workspace | State file + watchdog; compatibility list; if needed, the alternative of moving off-screen |
| The low-level mouse hook is frowned upon by some anti-cheats | The game does not start | The hook is removed in game mode; "Hold Alt to grab anywhere" can be turned off |
| The animated wallpaper method (`WorkerW`) breaks with a Windows update | Animated wallpaper is not visible | Both window structures are supported; on failure falls back to the static counterpart, the user is notified |
| A Windows update changes Explorer's drawing or XAML tree | The Explorer layer draws wrongly or crashes Explorer | Hooks only on tried builds; every hook falls back to the original; the crash guard turns the layer off; Explorer is restarted by Windows and comes back unthemed |
| An anti-cheat dislikes a modified Explorer | A game refuses to start or flags the user | Explorer only, never a game process; unloaded in game mode by default; tried by hand with Vanguard before the layer leaves experimental |
| A program writes its own file icon back | Some files show the old icon | Change notification on the keys, Kabuk's icon set again |
| The machine-wide folder icons need administrator rights | They cannot come back silently after a crash | They come back at the next start or `--restore` with the UAC prompt; the per-user layers come back at once |
| After Alt+drag, releasing Alt focuses the application menu | Annoying | Prevent menu activation by sending an unassigned virtual key when Alt is released (AutoHotkey's "mask key" method; `SendInput`, documented) |
