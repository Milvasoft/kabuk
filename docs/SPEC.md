# Kabuk: product specification

Visual counterpart: `docs/design/kabuk-tasarim.html`. If this file and the design conflict, this file wins.

## 1. Purpose and positioning

Kabuk brings the feel of Linux "rice" shells (Hyprland, i3, waybar) to Windows: a plain, fast, keyboard-driven desktop. The target user is a Windows user who cannot move to Linux (games, Office, Adobe, corporate tools) but wants that experience.

Positioning: **"The Hyprland feel, without the setup hassle, without breaking games."**

How we differ from the competition on Windows (komorebi, GlazeWM, Seelen UI, YASB, Zebar):

- One package: bar, window manager, launcher, notifications, screen saver and settings ship together.
- There is a settings screen; the text file is still fully authoritative and is live-reloaded.
- Looks good out of the box (7 built-in looks, light/dark mode).
- Crash-safe: under no circumstances is a window left hidden or the taskbar left missing.
- Works with games and anti-cheat without issues.

The two things the user will notice most are also the two things that, if missing, are enough for nobody to use the product: **a fluid feel** and **tiling that never breaks**. Priorities are set accordingly.

Target platform: Windows 11 22H2 and later. Windows 10 22H2 is supported on a best-effort basis (Windows 11-specific features such as rounded corners, DWM border colour and Mica are dropped there).

## 2. Components

### 2.1 Bar

- A separate bar on every monitor. Settings offer "All" or "Main only".
- Position: top, bottom, left, right. Every look has a default position (Ada, Çizgi, Devre, Rıhtım, Etkinlik, Izgara, Defter: top; Ray: left; Konsol, Pano: bottom). If the user changes it, "Use default" restores it.
- The bar reserves space from the Windows work area on the chosen edge; maximised windows do not go under the bar.
- In vertical positions (left, right) the clock is written stacked; the window title, date, media text and network speed are hidden.
- Modules (each can be turned on and off individually in settings; a disabled module leaves the bar immediately):

  | Key | Module | Note |
  | --- | --- | --- |
  | `title` | Title of the active window | |
  | `taskbar` | Apps | Pinned apps and the icons of open apps, as on the Windows taskbar (requested by the user on 2026-10-13, not on the design page; DECISIONS D25). A window belongs to the pin with its app id, as on Windows (a web app's pin holds the web app's windows, not the browser's; D51). A right click shows the app's jump list as Windows' taskbar does: pinned entries, the app's own categories, frequent or recent files, its tasks, then its name, pin/unpin, for a program its own rules (always open floating, always open on the workspace its window is on; D63) and close (requested by the user on 2026-10-14; D52). A file or picture dragged and held over an app's icon, on a bar or the dock, brings that app's window forward, as on Windows' taskbar (requested by the user on 2026-10-20; D86) |
  | `clock` | Clock | No seconds; 24/12-hour setting |
  | `date` | Date | Format depends on language ("Pzt 12 Eki") |
  | `media` | Playing media | Windows media session; play/pause; a right click shows the media card: cover, title, artist, album, previous, play/pause and next (requested by the user on 2026-10-15; DECISIONS D66) |
  | `cpu` | CPU usage | |
  | `ram` | RAM usage | GB, decimal separator depends on language |
  | `gpu` | GPU temperature | |
  | `net` | Network speed | |
  | `vol` | Volume | Changes with the scroll wheel, shows an OSD; a click opens the level, the output devices to choose from (D54), the apps playing with a level of their own (the volume mixer, D69) and Windows' sound settings; every level has a round thumb to drag |
  | `tray` | System tray | Left/right-click menus next to the icon. At most `tray_shown` icons (default 3) stay on the bar; the rest open in a panel from an overflow button, as on Windows (requested by the user on 2026-10-13; DECISIONS D26) |
  | `notif` | Notifications | Unread count badge |
  | `power` | Power menu | Lock, sleep, restart, shut down, sign out |
  | `app_grid` | All apps button | Opens the app grid (§2.5b); first on every bar (requested by the user on 2026-10-15; DECISIONS D56) |

- The workspace indicator is always present (not in the module list); clicking switches to that workspace, occupied workspaces are marked.
- Nothing on the bar covers anything else: the clock moves aside for the groups at the ends, and apps the bar has no room for (a bar at a side, a small monitor) go behind a "+n" that opens the app grid (found by the user: on a left bar the clock sat over the apps).
- The bar on the second and later monitors is minimal: workspaces, layout indicator, apps, clock, CPU and RAM. These modules also follow the global module setting. Every monitor's bar lists every app, as Windows' "show apps on all taskbars" (chosen by the user on 2026-10-15; DECISIONS D53).
- Auto-hide: when enabled the bar reserves no space and waits off its edge with 2 px left on screen; it slides in when the mouse rests at that edge for 0.25 s (a mouse passing the edge on its way to a window's top does not bring it) and slides off 0.6 s after the mouse left it (at once without full animations), but not while one of its panels or menus is open. Rıhtım's dock hides the same way at its own edge (DECISIONS D70). While shown, it pushes the windows (reserves its strip) and gives the room back when it slides off (D73); with full animations they slide with the bar, maximised ones too (D75).
- Background: solid (default, the desk colour over the bar's whole edge, as on the design's black desk), blur (the wallpaper blurred and tinted, as Windows' taskbar) or off (requested by the user on 2026-10-13; DECISIONS D28).
- Height: the bar's thickness can be set (24-72 px; default the look's own; DECISIONS D29).

### 2.2 Window manager

- Workspaces span every monitor (decided by the user on 2026-10-14, DECISIONS D48): workspace N is one workspace with each monitor's own windows on it, and switching (shortcut, bar, overview, launcher) takes every monitor there together. Default 1-9, the first 4 are shown; occupied ones (a window on any monitor) and the active one are always visible.
- Layouts: **dwindle** (default, grows by splitting in two as in Hyprland) and **master** (one main window, a stack beside it). Chosen per workspace.
- The window gap is adjustable (0-32 px, starting from the look's default).
- Direction of a new window: right, below, or floating.
- **Floating windows:**
  - Any window becomes floating with a single shortcut or by dragging it by its title ("Drag to float" setting).
  - With Alt held, a window is moved with left-click and resized with right-click from anywhere on it ("Hold Alt to grab anywhere").
  - Double-clicking the title of a floating window tiles it again (on windows with a title bar; replaces Windows' maximise behaviour, can be turned off in settings).
  - A workspace can run entirely in floating layout; "Floating" can be chosen as the default layout.
  - A click without movement does not float a window; it floats only if actually moved.
- The active window is framed with the accent colour (DWM border colour on Windows 11; can be turned off in settings).
- Smart floating: dialogs, splash screens, tooltips, owned windows, windows that cannot be resized by their border (they keep their own size; e.g. game launchers such as the League of Legends client, reported by the user) and windows whose minimum size does not fit the layout float by default.
- Windows running with administrator rights (which cannot be moved while Kabuk runs with normal rights) are marked floating, so the layout does not break.
- In later stages (see PLAN): hidden workspace (scratchpad), tabbed window groups, an overview showing all workspaces.

### 2.3 Window rules

- Rule: match (exe name, window class, title; plain text or regular expression) → action (floating, ignore, workspace N, monitor N, minimum width, placement preference).
- Default rules (the user can disable them): dialogs are always floating and centred; splash screens are ignored; `Taskmgr.exe` is floating.
- User rules are added from the settings screen; "Pick an open window" fills the form with the details of the clicked window.
- Rules are shown in the table as "Source: Default / Yours".

### 2.4 Game mode

- When a full-screen application (game, video player, presentation) opens, **within 1 second** on that monitor: the bar is hidden, tiling pauses (windows stay in place, new windows are not tiled), notifications are muted. A game at a resolution of its own counts (the monitor is taken as it is now), an overlay that clicks pass through does not hide it, and the bars are not made again under it when the resolution changes (DECISIONS D72).
- Other monitors work normally (can be changed in settings).
- Kabuk steps back for the game (DECISIONS D85): while the program is in front its keyboard hook is off (its mouse hook already is), its process runs below normal priority, nothing of Kabuk's is drawn or shown over the program (no hidden bar, dock or volume indicator), the wallpaper drift waits, and the program renaming itself costs nothing. Every monitor is looked at, with or without a bar of its own.
- Game mode belongs to the game's window (DECISIONS D87): another window in front of the game for a moment (a launcher, a notice, an app keeping itself on top) keeps it; one staying in front for 3 seconds ends it, and the game minimised, closed or out of full screen ends it at once. During a game Kabuk holds no GPU device of its own, raises no window over the game and leaves the look's change and the tray's re-registration of programs until the game is over.
- When the application closes, the previous layout returns exactly; muted notifications are shown as a single summary.
- Each behaviour can be turned off separately: automatic detection, hide the bar, pause tiling, mute notifications, keep other monitors normal.

### 2.5 Launcher

- Default shortcut `Win+R`. Depending on the look it opens centred (Ada, Devre, Rıhtım, Etkinlik, Defter), full-width under the bar (Çizgi), beside the rail (Ray), from the bottom like fzf (Konsol), at the bar's start corner like a start menu (Pano) or against the bar at its start corner (Izgara).
- Layout profiles (requested by the user on 2026-10-15; DECISIONS D65): the windows open now saved under a name (Settings › Windows), opened again in one go from the settings, the launcher or `kabuk msg profile <name>`: open programs go to their places, the rest start and their windows go there.
- Sources: open windows on all workspaces (selecting one switches to that workspace and focuses it; stands in for Alt+Tab), installed applications (Start menu shortcuts and Store apps), recent files, files and folders in indexed folders, browser bookmarks, system commands (lock, sleep, start screen saver, restart, shut down), calculator (`12*8` → result, Enter copies to the clipboard).
- Fuzzy search; matched letters are highlighted. Turkish case handling works correctly: searching "EKRANI" finds "Ekranı kilitle" (Lock screen), and the "ı/I" and "i/İ" distinction is preserved.
- Keyboard: up/down selects, Enter opens, Esc closes. Clicking outside closes it.
- If there are no results, help text is shown.

### 2.5b App grid (requested by the user on 2026-10-15; DECISIONS D56)

- Every app of Windows' "All apps" list on pages, as macOS's Launchpad: large icons with their names, alphabetical (Turkish order in Turkish), uninstallers and help or web links left out. Opened by `Win+Shift+A`, the bar's "All apps" button, a bar's "+n", or `kabuk msg apps`; on the monitor the user works on (the button's own monitor).
- It covers the monitor, over a blurred, darkened picture of the screen as it was (the wallpaper when the screen cannot be taken). The icons come in from slightly larger as they fade in and go back out the same way; pages slide.
- Pages: dots at the bottom (a click goes there), the wheel (either way), a drag with the mouse that follows it and turns the page past 15 % of its width (it gives only a little past the first and last page), Page Up/Down, Home/End. Arrow keys select an app and cross onto the next or previous page.
- Typing searches at once (the launcher's fuzzy matching, Turkish case rules); the best match is selected, Enter opens it, Esc clears the search, then closes. A click on an app opens it; a click anywhere else, or another window, closes the grid. Clicked or typed into, the search field shows it takes the keys: an accent frame and a caret; the text cursor shows over it (reported by the user on 2026-10-15).
- Editing (requested by the user on 2026-10-15; DECISIONS D59): a press held on an app, or a right click on it, begins it. Every app carries a "−" badge that hides it; a dragged app follows the mouse over the gap it will drop into, and held at a side edge it turns the page; Done (where the search was), Esc or a click between the apps ends it. The order and the hidden apps are kept; apps not in the order (new ones) come after it alphabetically. Settings › Launcher lists the hidden apps to show them again and puts the order back to alphabetical.
- Hot corners (requested by the user on 2026-10-15; DECISIONS D60): each corner of the screen can open the app grid, the overview or the launcher, or start the screen saver, when the mouse is pushed into it; nothing by default (Settings › Windows). Only a corner the mouse stops at counts (not one shared by two monitors); it acts once until the mouse leaves it, never during a drag, and not in game mode.
- Touchpad gestures (requested by the user on 2026-10-15; DECISIONS D68): with three (or four) fingers on a precision touchpad, a swipe left, right, up or down goes to the next or previous workspace, opens the overview, the app grid or the launcher, or does nothing, as chosen for each way (by default: left the next workspace, right the previous, up the overview, down the app grid). Off by default (Settings › Windows), with a button to Windows' touchpad settings, where Windows' own gestures for as many fingers must be set to "Nothing".

### 2.6 Notifications and OSD

- Kabuk's own notifications (e.g. "Window is floating", "Game mode on") are shown in a style matching the look and disappear by themselves.
- When the volume changes, the OSD (volume indicator) appears. The keyboard's volume keys are Kabuk's while it runs (they step the volume by 2 as Windows does), so Windows' own volume flyout does not show beside Kabuk's OSD (reported by the user on 2026-10-14; D50). In game mode they are Windows' again (while the window in front is on a full-screen monitor; elsewhere they stay Kabuk's).
- The notification badge on the bar shows the unread count of Windows notifications (technical risk: see ARCHITECTURE §Notifications).

### 2.7 Screen saver

- Chosen in settings: Milvasoft (circuit board eye), Constellation, None (turn the display off directly), or one of the gallery's (requested by the user on 2026-10-15; DECISIONS D58): ten live savers and ten still ones, each OLED-safe (mostly black, at most 8 % average brightness); a still one is drawn once and slowly wanders (2 % of the screen's height) and breathes (70-100 % light, 23 s), so nothing burns in.
- Wait time: 3, 5, 10, 15 min. "Don't start while video is playing" (honours Windows' display-on requests). "Preview" starts the selected screen saver immediately.
- The implementation is the existing `.scr` file in `reference/OledSaver`; Kabuk installs it and writes the Windows settings (`SCRNSAVE.EXE`, timeout, enabled).

### 2.8 Desktop wallpaper

- The "Wallpaper" page in settings, with two tabs: **Choose** and **Create**.
- Choose:
  - Monitor: All monitors, Monitor 1 (main), Monitor 2 … The choice applies only to the target monitor.
  - Animated: Circuit eye (the same scene as the screen saver), Constellation. Static: Windows' wallpaper (default: the user's own, untouched; changed by the user on 2026-10-13, as the design's default black desk is not what people use), Pure black, Night gradient (from the accent colour), Your image (a file is picked). Kabuk records the user's wallpaper before it sets another and gives it back when it exits. Created wallpapers are added to the list as "Created by you".
  - The gallery (requested by the user on 2026-10-15; DECISIONS D58): twelve live wallpapers (two of them Milvasoft's, made for the desktop) and ten still ones, for everyone, each with its preview. A live one is played as the animated ones are; a still one is drawn once at each monitor's size and becomes Windows' picture, recorded and given back like the others.
  - Brightness (10-100 %, default 60): the wallpaper's maximum brightness.
  - Pause when not needed (on by default): the animation stops while the desktop is fully covered by windows, in full screen, in game mode and on battery.
- Wallpapers are drawn with the look's tokens; when the look, mode or accent changes, the wallpaper follows.
- A static image drifts slowly enough not to be noticed (OLED).
- While a wallpaper is selected, bars that are normally transparent (except Ada's capsules) draw a background in the desktop colour; bar text is measured against this background, not the wallpaper, and the contrast rule is preserved.
- Create: provider (Claude, OpenAI, Ollama local), connection (API key in Windows Credential Manager; local address for Ollama), prompt text, Create button (disabled while not connected or while the prompt is empty). The steps appear in order: the model wrote the shader, compiled, performance (frame time at 4K, limit 4 ms), OLED (average brightness, static bright region). The result is previewed; Apply, Retry. A result that fails is sent back to the model with the error message, at most three attempts; if all three fail the reason is shown to the user. Format: §9.

### 2.9 Settings application

The 12 pages in the design, in the same order: Appearance, Wallpaper, Screen saver, Bar, Windows, Window rules, Game mode, Launcher, Shortcuts, Language & region, OLED care, About. Every change applies without a restart. Details are on the design page; page contents:

- **Appearance:** theme (7 cards with mini previews), accent colour (the theme's own colour + 7 colours: Ice blue `#8fd3ff`, Milva yellow `#fbc419`, Mint `#7fe0b5`, Coral `#ff8f6b`, Lilac `#b9a7ff`, Pink `#ff7eb6`, Off-white `#ecebe7`), mode (Dark, Light, Follow Windows, By time of day: light 07:00-19:00), text size (Small, Normal, Large), mouse cursors (the user's own; Kabuk's set in the theme's style and colour; or Kabuk's set in a style picked apart from the theme: Round, Slim, Circuit, Bold, Block, Cartoon, Comic, in Light, Dark or the accent colour, each card previewed; with Kabuk's sets a size, Normal, Large or Very large, which is Windows' own pointer size; requested by the user on 2026-10-13 and 2026-10-14, not on the design page, see DECISIONS D20 and D41). An OLED warning in light mode; if the accent colour was adapted for readability, it states which shade is used. The chosen cursors apply to the whole system while Kabuk runs; the user's previous cursors and pointer size are recorded first and come back when Kabuk exits or crashes. System theme: a switch per layer of §2.10, the icon layer with its set previewed for the theme in use, the Explorer layer with "On in game mode too" and, on a Windows version it was not tried on, "Try anyway" (requested by the user on 2026-10-14; drawn on the design page on 2026-10-14, waiting for the user's look). A theme file exports these looks' settings to share and imports one (requested by the user on 2026-10-15; DECISIONS D64).
- **Wallpaper:** §2.8.
- **Screen saver:** §2.7.
- **Bar:** position, monitors, auto-hide, background (solid, blur, off), colour (the look's own, a swatch or any colour; it fills the solid bar and tints the blur, with a note when the look's text would be hard to read on it; requested by the user on 2026-10-15, DECISIONS D57), height, tray icons shown on the bar, modules (§2.1).
- **Windows:** workspace layout (Tiling/Floating), float on drag, grab anywhere with Alt, window gap, new window direction, active window frame, windows follow the look (title bar colours and corners of other programs' windows, DECISIONS D30; requested by the user on 2026-10-13).
- **Window rules:** §2.3.
- **Game mode:** §2.4, anti-cheat note.
- **Launcher:** shortcut, search sources, indexed folders.
- **Shortcuts:** list of all shortcuts, each one changeable.
- **Language & region:** language (System language, Türkçe, English), time format (24/12), date and decimal separator preview.
- **OLED care:** bar shift, idle dimming, dimming delay (30 s, 1 min, 2 min, Custom: number + s/min, an integer between 10 s and 60 min; error message for an out-of-range value; the row is disabled while dimming is off).
- **About:** version, number of running components, number of monitors, number of managed windows, RAM usage, check for updates (Kabuk's latest release on GitHub, `Milvasoft/kabuk`; a newer one is offered with a button that opens its release page, whose setup installs over the running copy and keeps the settings).

### 2.10 System theme

Requested by the user on 2026-10-14 (DECISIONS D42): Windows itself follows Kabuk's look, in layers that are each turned on and off on their own (all off by default):

- **Colours:** Windows' app and system mode follow Kabuk's mode, and Windows' accent colour follows the look's accent (adapted by §4). When Kabuk's mode is "Follow Windows", Windows' mode is left alone and only the accent is set.
- **Icons** (dropped, 2026-10-14): giving folders, drives, This PC, the Recycle Bin and the common file kinds a per-look icon set was built and then removed. On Windows 11 the registry icon overrides are ignored (both the per-user `Explorer\CLSID` and the machine-wide `Shell Icons`), so nothing showed; the only working mechanisms are per-folder `desktop.ini` (cannot theme all folders) and patching `imageres.dll` (a risky system-file change Kabuk will not make). See DECISIONS D42.
- **Sounds:** none; Windows' own event sounds stay (decided by the user, DECISIONS D44).
- **Explorer** (dropped, 2026-10-14): drawing File Explorer's inside in the look's colours was built and then removed. On Windows 11 there is no safe, documented way to theme Explorer (its title bar and content are custom XAML); only the risky XAML/inline-hook path could, and the user chose not to take it (DECISIONS D42). Explorer keeps Windows' own look, which still follows dark/light through the colour layer.

Everything a layer changes is recorded first and comes back when the layer is turned off, when Kabuk exits or crashes, and with `kabuk --restore`; only the machine-wide folder icons wait for Kabuk's next start or `--restore` after a crash, because they need the administrator prompt.

## 3. Looks

Ten looks; each in dark and light mode. All colours are tokens (the `.screen[data-dir="x"]` and `.screen[data-mode="light"][data-dir="x"]` blocks on the design page). Detailed visuals: the design page.

| Code | Name | Essence | Default bar | Accent (dark / light) |
| --- | --- | --- | --- | --- |
| a | Ada (Island) | Floating capsules, rounded windows, breathing room | Top | `#8fd3ff` / `#1f6fd6` |
| b | Çizgi (Line) | Thin edge-to-edge strip, text instead of icons, hairline between windows; the least light | Top | `#ecebe7` / `#1c1b19` |
| c | Devre (Circuit) | Milva circuit board language: traces, pads, a via on the active window | Top | `#fbc419` / `#b58200` |
| d | Ray (Rail) | Solid vertical rail, stacked clock, launcher beside the rail | Left | `#ff8f6b` / `#d9552f` |
| e | Konsol (Console) | tmux status line, named workspaces, fzf launcher | Bottom | `#ff7eb6` / `#c2367a` |
| f | Rıhtım (Dock) | macOS-like: translucent menu bar, dock on the opposite edge, Spotlight launcher | Top | `#0a84ff` / `#0a7aff` |
| g | Etkinlik (Activities) | GNOME-like: pill workspace indicator, centred clock, single system menu; the bar is black in light mode too | Top | `#3584e4` / `#3584e4` |
| h | Pano (Panel) | A task-list panel: the apps as buttons with their names, the time over the date, a start-menu launcher | Bottom | `#4fd1c5` / `#0f766e` |
| i | Izgara (Grid) | Swiss grid: heavy capitals, workspace blocks the bar's height, a 3 px rule, square corners | Top | `#ff4b3e` / `#d4231b` |
| j | Defter (Notebook) | Editorial serif: Roman numerals, italics, small capitals, a double rule, a `·` between groups | Top | `#a8d5a2` / `#2f6b3a` |

The design page's eleventh look, Piksel (`k`), is not part of Kabuk (decided by the user, 2026-10-15; `DECISIONS.md` D67).

Default look: Rıhtım (`DECISIONS.md` D2).

## 4. Readability (contrast) rule

- In every look, every mode and every accent colour, all text has at least **4.5:1** contrast against its background (3:1 for text 24 px and above, or bold text 18.66 px and above). Measured with the WCAG 2.x relative luminance formula.
- The chosen accent colour is never applied as is. Two values are derived for each surface:
  - **text accent**: its lightness (HSL L) is changed until it reaches ≥4.5:1 against the backgrounds (desktop, panel, selected row, window, secondary surface); hue and saturation are preserved.
  - **fill accent** and **text on fill**: the theme's preferred text colour (black/white) if it is readable, otherwise the other one; if neither is readable, the fill is shifted.
- The same rule applies to all text roles: primary text, secondary text, in-app sample colours, notification badge. Even if a theme author supplies a wrong value, the result stays readable.
- Computed values target a `0.12` margin so that hexadecimal rounding does not push them below the limit.
- The working reference for the algorithm is the `fitContrast`, `fillPair`, `adaptAccent` functions on the design page; the native version must produce exactly the same result (unit tests are written with expected values taken from the design page).
- In every release all look × mode × accent combinations are measured automatically; if a theme fails the limit it is not loaded, the last valid theme stays and the user is notified.
- Disabled elements and the idle-dimmed state are exempt from this rule.

## 5. OLED rules

- Dark mode by default; the bar and desktop background are pure black.
- Bar shift: shifts 1-2 pixels every few minutes.
- Idle dimming: if there is no activity for the configured time (default 1 min), the bar and windows are dimmed; they return on mouse/keyboard input. No dimming while media is playing or in a full-screen application. In light mode dimming starts earlier.
- The accent colour is used only for state (active workspace, active window, warning); no colour on static decoration.
- No seconds on the clock.
- The bar auto-hides if desired.

## 6. Shortcuts

Defaults are on the Win key (the user's decision on 2026-10-14, DECISIONS D40: Alt chords clashed with programs' own, and a chord another program had registered never reached Kabuk). Kabuk reads them with a low-level keyboard hook before Windows and other programs. Win+L (lock) and Ctrl+Alt+Del cannot be captured; Windows' Win chords that people use (Win+E, Win+V, Win+Shift+S) and the input-language switch (Win+Space) are left alone; the Win chords below lose their Windows meaning while Kabuk runs.

| Action | Default |
| --- | --- |
| Open launcher | `Win+R` |
| Switch to workspace | `Win+1…9` |
| Move window to workspace | `Win+Shift+1…9` |
| Focus: left, down, up, right | `Win+arrow keys` |
| Move window in direction | `Win+Shift+arrow keys` |
| Float / tile window | `Win+F` |
| Move window | `Alt` + left drag |
| Resize window | `Alt` + right drag |
| Close window | `Win+Shift+Q` |
| Change layout (dwindle/master) | `Win+Y` |
| Show / hide the hidden workspace (scratchpad) | `Win+Z` |
| Send window to the hidden workspace | `Win+Shift+Z` |
| Make a tabbed group / break it up | `Win+T` |
| Next tab of the group | `Win+Shift+T` |
| Overview of every workspace | `Win+Tab` |
| Start the screen saver now (the displays turn off when no saver is chosen) | `Win+Shift+L` (requested by the user on 2026-10-14) |
| Every app on a grid (§2.5b) | `Win+Shift+A` (requested by the user on 2026-10-15) |
| Drop-down terminal (from the top of the monitor, over every workspace; DECISIONS D62) | `Win+Enter` (requested by the user on 2026-10-15) |
| The card of the main shortcuts (also a greeting on the first start; DECISIONS D61) | `Win+F1` (requested by the user on 2026-10-15) |
| Emergency exit and restore | `Ctrl+Alt+Shift+End` (cannot be changed) |

On the Turkish Q keyboard there must be no conflict with AltGr (`Ctrl+Alt`) or the input-language switch shortcuts (`Alt+Shift`, `Win+Space`). Pressing and releasing `Alt+Shift` on its own must not break input-language switching.

## 7. Language and formats

- All text comes from translation files (`locales/tr.ftl`, `locales/en.ftl`). The initial strings are taken from the `L.tr` and `L.en` dictionaries on the design page.
- On first launch, the Windows display language; in settings, System/Türkçe/English. The change applies without a restart.
- Date, time and decimal formats follow the language (TR: "Pzt 12 Eki", "9,8"; EN: "Mon 12 Oct", "9.8"). In 12-hour format TR uses "ÖÖ/ÖS", EN "AM/PM".
- Turkish case rules in search and sorting.
- Monitors are nowhere referred to as "right/left monitor": "Monitör 1 (ana)" (Monitor 1 (main)), "Monitör 2"…; the arrangement is read from Windows.

## 8. Acceptance criteria

Each is measurable; each is verified in every release with the tests in PLAN.

1. **Crash safety.** If Kabuk crashes or is killed from Task Manager, the watchdog process restores all hidden windows and the taskbar and resets the work area within 2 seconds. If the bar or the window manager stops answering for 15 seconds, the watchdog ends Kabuk and does the same, then starts it again (DECISIONS D55).
2. **If Explorer restarts**, the bar re-establishes its reserved space, the tray icons and the window layout by itself.
3. **Two monitors.** When a monitor is unplugged and plugged back in, and on wake from sleep, windows stay on their own monitor. On monitors with different DPI the bar and text are sharp on every screen.
4. **Smart floating windows** (§2.2) do not break the layout.
5. **Game mode** engages within 1 second; when it ends, the layout returns exactly.
6. **Anti-cheat compatibility.** No code is injected into other processes; only documented APIs. (The opt-in Explorer layer of §2.10 would have been the one exception; it was dropped, so Kabuk now injects into no process at all.)
7. **Shortcuts** do not conflict with Windows' reserved shortcuts or with the Turkish keyboard; all are changeable.
8. **System tray.** All tray icons are visible, menus open next to the icon.
9. **Administrator windows** do not silently break; they are marked floating. If desired, Kabuk can be started as administrator.
10. **Resource usage.** Average CPU below 0.1% over ten minutes idle. RAM is measured in every release and shown in About; target below 40 MB for bar+WM+launcher (excluding the settings application; it runs only while open), plus the smooth genie's GPU renderer (~49 MB), kept while the genie is on (chosen by the user on 2026-10-17; DECISIONS D83).
11. **Response time.** Below 16 ms from shortcut to the window layout being applied (without animation); launcher below 100 ms from shortcut to first frame.
12. **Live settings.** Every change applies without a restart; also when `kabuk.toml` is edited by hand.
13. **Floating windows** (§2.2).
14. **Bar position** works on all four edges, and the work area is updated accordingly.
15. **Multilingual interface** (§7).
16. **Light and dark mode** in all ten looks, with the four mode options.
17. **Bar modules** turn on and off individually, reflected instantly.
18. **Readability** (§4).
19. **Fluid feel.** Kabuk's own surfaces (bar, launcher, notification, OSD) animate at 60 fps or more without dropping frames; window layout transitions do not stutter (see ARCHITECTURE §Animation). Animation setting Full/Reduced/Off; off if Windows' "Animation effects" is off.
20. **Tiling never breaks.** Every application in the problem-application test list (PLAN §Test) is tried automatically in every release; none of them shifts the layout, overlaps windows or leaves a window invisible.
21. **Wallpaper.** The animated wallpaper spends no CPU/GPU while the desktop is covered, in full screen and on battery; while visible, GPU time per frame at 4K < 4 ms, capped at 30 fps. When Kabuk closes, the user's previous Windows wallpaper is restored.
22. **Clean exit.** When Kabuk is closed, the taskbar, work area, window positions and changed Windows settings return to their previous state.
23. **Command line.** Every action can be triggered externally with `kabuk msg <action>`; state can be queried as JSON.
24. **System theme** (§2.10). Each layer turns on and off live; turned off, after an exit, a crash or `--restore`, Windows' colours, icons and File Explorer look exactly as before (the machine-wide folder icons after a crash: at the next start or `--restore`). Explorer never stays with Kabuk's code loaded after Kabuk has ended.

## 9. Roadmap (after the first release)

- **Wallpaper format (for Create and plugins):** a folder or a `.kabukwall` archive: `wall.toml` (name, author, version, adjustable values and their ranges, tokens used) + `wall.hlsl` (a single pixel shader; inputs: time, resolution, theme tokens, adjustable values; no access to anything else such as files, network or the clock). Checks: compiles, frame time at 4K < 4 ms, black background (in dark mode), average brightness ≤ 8% and no bright region that stays static for 60 seconds. Wallpaper page in M12, Create in M13 (see PLAN).
- **Plugins:** WebAssembly components (wasmtime), capability-based permissions (network, file, process info). A plugin does not draw its own UI; it returns data and a small layout description, and Kabuk draws it with the theme's colours and the contrast rule. Extension points: bar modules, launcher sources and commands, wallpapers, window rules. Built-in modules are also written to use the same interface (see ARCHITECTURE §Module interface).
- **Theme file sharing and gallery.**
