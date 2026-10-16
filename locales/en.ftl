## Kabuk — English

## Command line

cli-usage =
    Usage:
      kabuk                   Starts Kabuk
      kabuk --restore         Emergency restore: stops Kabuk, shows hidden windows and brings the taskbar back
      kabuk --install         Installs Kabuk for you (no administrator rights needed)
      kabuk --uninstall       Undoes everything and removes Kabuk; your settings stay
      kabuk --startup on|off  Starts Kabuk when you sign in, or not
      kabuk msg <command>     Sends a command to the running Kabuk:
                                workspace <1-9>, move-to <1-9>, focus <left|down|up|right>,
                                float toggle, scratchpad <toggle|send>,
                                layout <dwindle|master|floating|tiling>, reload,
                                query <state|windows|monitors>, launcher, overview, saver, apps,
                                restore
    Options:
      --state-dir <folder>    Folder for the state file and logs
      --config <file>         Configuration file
cli-error-unknown = Unknown argument: { $arg }
cli-error-missing-value = { $flag } needs a value.
cli-error-bad-value = Invalid value for { $flag }: { $value }
cli-error-not-with-restore = { $flag } cannot be used with --restore.
cli-error-not-with-msg = { $flag } cannot be used with msg.
cli-error-not-with-setup = { $flag } cannot be used with --install, --uninstall or --startup.
cli-msg-empty = A command is needed.
cli-msg-unknown = Unknown command: { $command }
cli-msg-bad-args = Wrong arguments for { $command }.
cli-msg-not-running = Kabuk is not running.
cli-msg-unreadable = Kabuk's answer could not be read.
ipc-error-no-wm = The window manager is off.
ipc-error-no-bar = The bar is off.
ipc-error-bad-request = The command was not understood.
cli-error-debug-unavailable = { $flag } is only available in the test build.
cli-restore-done =
    { $windows ->
        [0] Restored. There were no hidden windows.
        [one] Restored: 1 window shown.
       *[other] Restored: { $windows } windows shown.
    }
cli-restore-state-unreadable = The state file could not be read ({ $path }). The taskbar was shown anyway; hidden windows could not be found.

## Startup

startup-failed-title = Kabuk could not start
startup-failed-state = The state file cannot be written: { $path }. Kabuk closed without hiding any window.

## Watchdog

watchdog-gave-up-title = Kabuk stopped
watchdog-gave-up-body =
    Kabuk crashed { $count } times within { $seconds } seconds and will not be restarted. Hidden windows and the taskbar were brought back.

    Details: { $log }

## Dates, times and numbers
## Day and month names come from locales/design (the design page).

date-short = { $weekday } { $day } { $month }
decimal-separator = {"."}

## Bar

bar-net-rate = { $value } MB/s
bar-percent = { $value }%
bar-ram = { $value }G
bar-temp = { $value }°
bar-tray-more = +{ $value }

## Konsol names its first workspaces like tmux windows (from the design page's CSS).

konsol-ws-1 = :code
konsol-ws-2 = :web
konsol-ws-3 = :term
konsol-ws-4 = :docs
konsol-ws-5 = :chat
konsol-ws-6 = :misc

## Launcher

launcher-kind-window = Window
launcher-window-where = Workspace { $workspace }, Monitor { $monitor }
launcher-restart = Restart
launcher-shutdown = Shut down
launcher-settings = Kabuk settings
power-sign-out = Sign out
vol-mixer = Open volume mixer
vol-apps = Volume mixer
vol-settings = Sound settings
vol-troubleshoot = Troubleshoot sound problems
clock-adjust = Adjust date and time
notif-settings = Notification settings
notif-focus = Do not disturb
stats-task-manager = Task Manager
stats-resource-monitor = Resource Monitor
net-settings = Network and internet settings
net-troubleshoot = Troubleshoot network problems
app-pin = Pin to the bar
app-unpin = Unpin from the bar
app-float-rule = Always open floating
app-float-rule-off = Stop opening floating
app-ws-rule = Always open on workspace { $value }
app-ws-rule-off = Stop opening on workspace { $value }
app-close = Close window
app-close-all = Close all windows
app-recent = Recent
app-pinned = Pinned
app-frequent = Frequent
app-tasks = Tasks

## App grid (every app on pages, as macOS's Launchpad)

grid-search = Search
grid-empty = No app matches
grid-done = Done
lp-grid = All apps grid
lp-grid-title = All apps (Launchpad)
media-nothing = Nothing plays
k-profile = Layout profile
k-profile-open = Open the { $value } layout
pf = Layout profiles
pf-h = Your programs over monitors and workspaces, saved from the windows open now and opened again in one go (also from the launcher)
pf-count = { $value } programs
pf-open = Open
pf-none = No layout profiles yet
pf-name = Profile name
pf-name-ph = Coding, Meeting…
pf-save = Save the layout now
pf-name-missing = Give the profile a name first
pf-empty = No window to save: Kabuk is not running, or no program is open
pf-not-running = Kabuk is not running
pf-opened = Opening; programs not open yet start and take their places
th = Theme file
th-h = Your look, colours, bar, cursors and gallery wallpaper in one file to share; a theme never changes shortcuts, rules or programs
th-export = Export…
th-import = Import…
th-files = Kabuk theme
th-exported = Saved: { $value }
th-imported = Theme applied: { $value }
th-failed = This file is not a Kabuk theme this Kabuk can read
dd = Drop-down terminal
dd-h = It drops down from the top of the monitor and hides again with its shortcut, over every workspace
dd-command = Program
dd-command-h = With its arguments; empty: Windows Terminal, else PowerShell
dd-command-ph = Windows Terminal
dd-height = Height
set-key-dropdown = Drop-down terminal
hc = Hot corners
hc-h = Push the mouse into a corner of the screen to do something; a corner between two monitors does nothing
hc-top-left = Top left
hc-top-right = Top right
hc-bottom-left = Bottom left
hc-bottom-right = Bottom right
hc-none = Nothing
hc-app-grid = All apps
hc-overview = Every workspace (overview)
hc-launcher = Launcher
hc-saver = Screen saver
gs = Touchpad gestures
gs-h = Swipe with three or four fingers on a precision touchpad; first set Windows' own gestures for as many fingers to "Nothing", or both happen
gs-on = Swipes
gs-on-h = Off by default
gs-fingers = Fingers
gs-left = Swipe left
gs-right = Swipe right
gs-up = Swipe up
gs-down = Swipe down
gs-next = Next workspace
gs-previous = Previous workspace
gs-windows = Windows' touchpad settings
lp-grid-h = Apps you hid with the − badge; hold an app on the grid, or right-click it, to move or hide apps
lp-grid-none = No hidden apps
lp-grid-show = Show
lp-grid-reset = Back to alphabetical order
grid-edit-h = Drag an app to move it · − hides it · Settings › Launcher shows it again

## Gallery (DECISIONS D58): each design's name and words

gal-walls-live = Gallery · live wallpapers
gal-walls-still = Gallery · still wallpapers
gal-savers-live = Gallery · live screen savers
gal-savers-still = Gallery · still screen savers
gal-still-saver-h = A still picture that slowly drifts and breathes, so nothing burns in
gal-walls-as-savers = Gallery · live wallpapers as screen savers
gal-wall-saver-h = Brighter than the screen savers: not for OLED screens
gal-wall-live-aurora-shore = Aurora shore
gal-wall-live-aurora-shore-h = Northern lights over a still lake
gal-wall-live-contour-breath = Contour breath
gal-wall-live-contour-breath-h = A dark map whose hills slowly breathe
gal-wall-live-ink-bloom = Ink bloom
gal-wall-live-ink-bloom-h = Indigo ink unfurling in still water
gal-wall-live-leaf-shallows = Leaf shallows
gal-wall-live-leaf-shallows-h = Sunlight weaving over a sea floor
gal-wall-live-milva-desk = Milva circuit board
gal-wall-live-milva-desk-h = The Milvasoft eye on a quiet board
gal-wall-live-milva-desk-2 = Milva relief
gal-wall-live-milva-desk-2-h = The Milvasoft eye in graphite, a warm light circling
gal-wall-live-milva-desk-3 = Milva constellation
gal-wall-live-milva-desk-3-h = The Milvasoft eye drawn in gold stars on a night sky
gal-wall-live-milva-desk-4 = Milva liquid gold
gal-wall-live-milva-desk-4-h = The Milvasoft eye in molten gold, ink rings drifting out
gal-wall-live-milva-desk-5 = Milva beacon
gal-wall-live-milva-desk-5-h = The Milvasoft eye as a lighthouse over a night sea
gal-wall-live-milva-desk-6 = Milva Bauhaus
gal-wall-live-milva-desk-6-h = The Milvasoft eye in a flat poster of yellow and graphite
gal-wall-live-paper-dunes = Paper dunes
gal-wall-live-paper-dunes-h = Cut-paper dunes under a drifting sun
gal-wall-live-quiet-orrery = Quiet orrery
gal-wall-live-quiet-orrery-h = Minimal planets on tilted orbits
gal-wall-live-rain-glass-city = Rain on the glass
gal-wall-live-rain-glass-city-h = A night city blurred behind raindrops
gal-wall-live-seigaiha-tide = Seigaiha tide
gal-wall-live-seigaiha-tide-h = Japanese wave scales rippling in indigo
gal-wall-live-silk-loom = Silk loom
gal-wall-live-silk-loom-h = Satin ribbons weaving over and under
gal-wall-live-velvet-mesh = Velvet mesh
gal-wall-live-velvet-mesh-h = Soft pools of colour folding into each other
gal-wall-live-iznik-glaze = Iznik glaze
gal-wall-live-iznik-glaze-h = Ottoman tiles turning slowly under a glaze of light
gal-wall-live-ebru-flow = Ebru flow
gal-wall-live-ebru-flow-h = Marbled inks combed into waves that slowly drift
gal-wall-live-chrome-drift = Chrome drift
gal-wall-live-chrome-drift-h = Drops of liquid metal merging and parting
gal-wall-live-riso-drift = Riso drift
gal-wall-live-riso-drift-h = A two-ink halftone print of sun and hills, slowly shifting
gal-wall-live-paper-pleats = Paper pleats
gal-wall-live-paper-pleats-h = Curved paper folds breathing under a soft lamp
gal-wall-live-pine-mist = Pine mist
gal-wall-live-pine-mist-h = Pine ridges fading into drifting dawn mist
gal-wall-live-cloud-sea = Cloud sea
gal-wall-live-cloud-sea-h = Rolling clouds at dawn, seen from a summit
gal-wall-live-obsidian-seams = Obsidian seams
gal-wall-live-obsidian-seams-h = Volcanic glass with slowly glowing seams
gal-wall-live-wheat-wind = Wheat wind
gal-wall-live-wheat-wind-h = Slow gusts rolling over a golden wheat field
gal-wall-live-desert-night = Desert night
gal-wall-live-desert-night-h = Moonlit dunes under the slowly turning Milky Way
gal-wall-still-agate-slice = Agate slice
gal-wall-still-agate-slice-h = A polished blue agate on slate
gal-wall-still-bauhaus-afternoon = Bauhaus afternoon
gal-wall-still-bauhaus-afternoon-h = A printed Bauhaus poster on cream paper
gal-wall-still-clay-cairn = Clay cairn
gal-wall-still-clay-cairn-h = Pastel clay stones in balance
gal-wall-still-ginkgo-lines = Ginkgo in gold
gal-wall-still-ginkgo-lines-h = Gold line art on forest green
gal-wall-still-isometric-isle = Isometric isle
gal-wall-still-isometric-isle-h = A tiny cube-world island, floating
gal-wall-still-light-ribbon = Light ribbons
gal-wall-still-light-ribbon-h = Translucent ribbons of violet light
gal-wall-still-low-poly-dusk = Low poly dusk
gal-wall-still-low-poly-dusk-h = Faceted mountains at sunset
gal-wall-still-paper-hills-morning = Paper hills
gal-wall-still-paper-hills-morning-h = A misty morning valley of cut paper
gal-wall-still-riso-ringed-planet = Riso planet
gal-wall-still-riso-ringed-planet-h = A ringed planet in two-ink risograph
gal-wall-still-rose-silk-folds = Rose silk
gal-wall-still-rose-silk-folds-h = Rose and wine silk in deep folds
gal-saver-live-abyssal-jellies = Abyssal jellies
gal-saver-live-abyssal-jellies-h = Glowing jellyfish rising in the deep
gal-saver-live-comet-calligraphy = Comet calligraphy
gal-saver-live-comet-calligraphy-h = A comet writing slow cursive loops
gal-saver-live-firefly-meadow = Fireflies over the meadow
gal-saver-live-firefly-meadow-h = Fireflies flashing above dark hills
gal-saver-live-frost-bloom = Frost bloom
gal-saver-live-frost-bloom-h = Snow crystals growing and melting
gal-saver-live-light-petals = Falling light petals
gal-saver-live-light-petals-h = Cherry petals of soft pink light
gal-saver-live-neon-rosettes = Neon rosettes
gal-saver-live-neon-rosettes-h = A neon pen drawing rose curves
gal-saver-live-night-rain-pond = Night rain on the pond
gal-saver-live-night-rain-pond-h = Raindrops ringing a moonlit pond
gal-saver-live-quiet-orrery = Quiet orrery
gal-saver-live-quiet-orrery-h = Planets circling a breathing sun
gal-saver-live-smoke-rings = Smoke rings of light
gal-saver-live-smoke-rings-h = Luminous rings rising and fraying
gal-saver-live-wireframe-galaxy = Wireframe galaxy
gal-saver-live-wireframe-galaxy-h = A mesh spiral galaxy turning slowly
gal-saver-still-celestial-chart = Celestial chart
gal-saver-still-celestial-chart-h = An old star map drawn in light
gal-saver-still-dandelion-clock = Dandelion clock
gal-saver-still-dandelion-clock-h = A seed head with seeds sailing away
gal-saver-still-deep-sea-jellyfish = Deep-sea jellyfish
gal-saver-still-deep-sea-jellyfish-h = A glassy jellyfish lit from within
gal-saver-still-ember-enso = Ember enso
gal-saver-still-ember-enso-h = A single vermilion brush circle
gal-saver-still-kintsugi-bowl = Kintsugi bowl
gal-saver-still-kintsugi-bowl-h = A black bowl mended with gold
gal-saver-still-light-painted-spiral = Light-painted spiral
gal-saver-still-light-painted-spiral-h = Coloured lights swung in a spiral
gal-saver-still-moon-on-still-water = Moon on still water
gal-saver-still-moon-on-still-water-h = A crescent over a calm silver sea
gal-saver-still-neon-arcade = Neon arcade
gal-saver-still-neon-arcade-h = Neon arches mirrored in a wet floor
gal-saver-still-ringed-planet-nightside = Ringed planet, night side
gal-saver-still-ringed-planet-nightside-h = A gas giant lit from behind
gal-saver-still-three-fireflies = Three fireflies
gal-saver-still-three-fireflies-h = A long exposure of three flights

## Notices

notify-missed = { $value } notices came while it was on
launcher-calc = { $query } = { $value }

## Settings app (what the design page does not word)

set-pick-images = Images
set-pick-folder = Choose a folder
set-refused = This value can't be saved
set-px = { $value } px
set-remove = Remove
set-add = Add
set-cancel = Cancel
mod-taskbar = Apps
mod-app-grid = All apps button
set-bg = Background
set-bg-h = What lies behind the bar over the wallpaper
set-bg-solid = Solid
set-bg-blur = Blurred
set-bg-off = None
set-height = Height
set-height-h = The bar's thickness
set-height-default = Theme's own: { $value } px
set-tray-shown = Tray icons on the bar
set-tray-shown-h = The rest open from the overflow button
set-rule-exe = Program
set-rule-exe-ph = e.g. Discord.exe
set-rule-open = Open programs
set-rule-what = Rule
set-rule-float = Floating
set-rule-ignore = Ignore, never tile
set-rule-workspace = Workspace { $value }
set-rule-monitor = Monitor { $value }
set-rule-min-width = at least { $value } px
set-rule-no-grab = Keeps its own Alt + click
set-rule-none = No rules of your own yet
set-game-note = Kabuk never injects code into games and installs no global hooks; anti-cheat systems see an ordinary window.
set-src-windows = Open windows
set-src-apps = Apps
set-src-recent = Recent files
set-src-files = Files in indexed folders
set-src-bookmarks = Bookmarks
set-src-commands = Commands
set-src-calculator = Calculator
set-folder-add = Add folder
set-key-launcher = Open launcher
set-key-workspace = Go to workspace (with 1…9)
set-key-move-to-workspace = Move window to workspace (with 1…9)
set-key-focus-left = Focus left
set-key-focus-down = Focus down
set-key-focus-up = Focus up
set-key-focus-right = Focus right
set-key-move-left = Move window left
set-key-move-down = Move window down
set-key-move-up = Move window up
set-key-move-right = Move window right
set-key-toggle-float = Float or tile window
set-key-close-window = Close window
set-key-cycle-layout = Switch layout
set-key-toggle-scratchpad = Show or hide the hidden workspace
set-key-to-scratchpad = Send the window to the hidden workspace
set-key-toggle-group = Make a tabbed group / break it up
set-key-next-tab = Next tab of the group
set-key-overview = Overview of every workspace
set-key-start-saver = Start the screen saver now
set-key-keys-card = Show the shortcut card
keys-title = Shortcuts
keys-title-h = Kabuk's main shortcuts, as you set them
keys-welcome = Welcome to Kabuk
keys-welcome-h = Your windows tile by themselves. These shortcuts are all you need to start.
keys-foot = Any key or click closes the card · Settings › Shortcuts changes them, this card's too
keys-g-windows = Windows
keys-g-workspaces = Workspaces
keys-g-kabuk = Kabuk
keys-focus = Move the focus
keys-move = Move the window
keys-workspace = Go to a workspace
keys-move-to = Send the window to a workspace
keys-emergency = Emergency exit (always works)
set-key-app-grid = Every app on a grid
set-bar-color = Bar colour
set-bar-color-h = Fills the bar when its background is solid or blurred
set-bar-color-own = The look's own
set-bar-color-custom = Any colour
set-bar-color-low = The look's text is hard to read on this colour
set-key-drag = Move with the mouse (right button resizes)
set-key-press = Press the new shortcut…
set-reset-keys = Reset all shortcuts to their defaults
set-key-mods = Press the keys to hold…
set-version = Kabuk { $value }
set-mode-dark-h = The theme's dark colours, all day
set-mode-light-h = The theme's light colours, all day
set-mode-system-h = Dark or light, as Windows is set
set-mem = MB memory
set-not-running = Kabuk is not running
set-quit = Quit Kabuk
set-quit-h = The bar and the window manager stop; hidden windows, the taskbar and everything Kabuk changed come back
set-quit-btn = Quit
set-update-checking = Checking GitHub for a newer Kabuk…
set-update-latest = Kabuk is up to date
set-update-newer = Kabuk { $value } is out: download its setup and run it; it installs over this one and keeps your settings
set-update-get = Open the release
set-update-failed = GitHub could not be reached; try again later
set-cursors = Mouse cursors
set-cursors-h = Kabuk's cursors follow the theme or your pick; yours come back when Kabuk exits
set-cursors-windows = Your own
set-cursors-kabuk = The theme's set
set-cursors-custom = My pick
set-cursor-style = Cursor style
set-cursor-color = Cursor colour
set-cursor-size = Cursor size
set-cursor-size-h = Windows' own pointer size while Kabuk's cursors are in use
cursor-style-round = Round
cursor-style-slim = Slim
cursor-style-circuit = Circuit
cursor-style-bold = Bold
cursor-style-block = Block
cursor-style-cartoon = Cartoon
cursor-style-comic = Comic
cursor-color-light = Light
cursor-color-dark = Dark
cursor-color-accent = Accent
cursor-size-normal = Normal
cursor-size-large = Large
cursor-size-huge = Very large
set-animations = Animations
set-animations-h = Off while Windows' animation effects are off, and in game mode
set-animations-full = Full
set-animations-reduced = Reduced
set-animations-off = Off
set-sys-theme = System theme
set-sys-colors = Colours
set-sys-colors-h = Windows' mode and accent colour follow the theme; yours come back when Kabuk exits
set-sys-colors-follow = Windows' accent colour follows the theme; its mode stays yours, as the theme follows it
set-drift = Your still picture drifts
set-drift-h = Your own wallpaper picture moves by two pixels every { $value } minutes, too little to see, so no pixel stays lit the same way
set-follow-look = Windows follow the theme
set-follow-look-h = Other apps' title bars take the theme's colours and corners
set-minimize = Minimise effect
set-minimize-h = How windows go into their icon on the bar and come back; with animations in full
set-minimize-genie = Genie
set-minimize-windows = Windows'
tabs-untitled = Untitled window
w-windows = Windows' wallpaper
w-windows-h = Your own · Kabuk leaves it alone
day-long-0 = Sunday
day-long-1 = Monday
day-long-2 = Tuesday
day-long-3 = Wednesday
day-long-4 = Thursday
day-long-5 = Friday
day-long-6 = Saturday
month-long-0 = January
month-long-1 = February
month-long-2 = March
month-long-3 = April
month-long-4 = May
month-long-5 = June
month-long-6 = July
month-long-7 = August
month-long-8 = September
month-long-9 = October
month-long-10 = November
month-long-11 = December
date-long = { $weekday }, { $day } { $month }
saver-settings-elsewhere = The screen saver's settings are in Kabuk Settings, on the Screen saver page.

## Create (the Wallpaper page's Create tab)

gen-name-claude = Claude
gen-name-openai = OpenAI
gen-name-ollama = Ollama
gen-why-reply = the answer held no wallpaper files
gen-why-format = its files did not follow the wallpaper format
gen-why-compile = the shader did not compile
gen-why-slow = a frame took { $value } ms at 4K (the limit is 4 ms)
gen-why-draw = it could not be drawn on this computer's GPU
gen-why-black = the background was not black
gen-why-bright = it was too bright on average ({ $value } %, the limit is 8 %)
gen-why-static = a bright area stayed in one place
gen-fail-no-key = Add an API key first
gen-fail-key = The API key was refused
gen-fail-no-model = No model is installed in Ollama
gen-fail-network = { $value } could not be reached
gen-fail-api = The model answered with an error ({ $value })
gen-fail-unreadable = { $value }'s answer could not be read
gen-fail-refused = Three tries did not pass. Last reason: { $value }
gen-fail-cancelled = Stopped
gen-key-ph = Paste the API key
gen-key-save = Save key
gen-try = Try { $value } of 3
gen-step-perf = Performance: { $value } ms per frame at 4K (limit 4 ms)
gen-step-oled = OLED: average brightness { $value } %, no bright area stays still
gen-note = The model writes the wallpaper; Kabuk checks it and sends one that fails back to be fixed, up to three times.
gen-stop = Stop
gen-ollama-missing = Ollama did not answer at { $value }
gen-ollama-model = Connected · local model { $value }

## Installing (kabuk --install, --uninstall, --startup)

setup-title = Kabuk
setup-shortcut-settings = Kabuk Settings
setup-ask-startup = Should Kabuk start every time you sign in to Windows? You can change this later with "kabuk --startup on" or "off".
setup-installed = Kabuk is installed in { $path }. Start it from the Start menu.
setup-failed = Kabuk could not be installed: { $error }
setup-removed = Kabuk was removed and Windows is as it was. Your settings stay in { $path }.
setup-remove-failed = Kabuk could not be removed completely: { $error }
setup-startup-on = Kabuk will start when you sign in.
setup-startup-off = Kabuk will not start when you sign in.
setup-confirm = Kabuk will be installed for you (no administrator rights needed). While it runs, its own bar takes the place of Windows' taskbar and it arranges your windows; when it exits, Windows is as it was. If anything ever goes wrong, Ctrl+Alt+Shift+End brings everything back. Install Kabuk now?
setup-done = Kabuk is installed and running. "Kabuk Settings" in the Start menu changes its look and behaviour; the "Kabuk" shortcut on your desktop starts it again. In a terminal, "kabuk --help" lists its commands. Ctrl+Alt+Shift+End brings Windows back at any time.
setup-no-payload = This setup holds no programs (it was built without them). Build it with tools\package.ps1.
