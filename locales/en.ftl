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
                                query <state|windows|monitors>, launcher, overview, saver,
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
app-close = Close window
app-close-all = Close all windows
app-recent = Recent
app-pinned = Pinned
app-frequent = Frequent
app-tasks = Tasks

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
