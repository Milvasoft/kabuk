//! The `kabuk.toml` schema. Every key maps to a setting in `docs/SPEC.md`; defaults
//! follow the spec and the design page. Values that need checks beyond their type
//! (ranges, colours, shortcuts, regexes) are validated in `validate.rs`.

use serde::{Deserialize, Serialize};

/// Current schema version written to `schema = N`.
pub const SCHEMA_VERSION: u32 = 1;

/// The look used on first start (DECISIONS D2; Rıhtım since 2026-10-14, asked for by the
/// user).
pub const DEFAULT_LOOK: Look = Look::Rihtim;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    pub appearance: Appearance,
    pub wallpaper: Wallpaper,
    pub screensaver: Screensaver,
    pub bar: Bar,
    pub windows: Windows,
    pub rules: Rules,
    pub game: Game,
    pub launcher: Launcher,
    pub shortcuts: Shortcuts,
    pub language: Language,
    pub oled: Oled,
    pub create: Create,
    pub system_theme: SystemTheme,
    pub hot_corners: HotCorners,
    pub dropdown: Dropdown,
    pub gestures: Gestures,
    pub notices: Notices,
    /// Layout profiles (DECISIONS D65), `[[profile]]`.
    #[serde(rename = "profile", skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<Profile>,
}

/// A layout profile (`kabuk_core::profile`): programs over monitors and workspaces.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    #[serde(rename = "window", skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<ProfileWindow>,
}

/// A program's place in a layout profile.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileWindow {
    /// Its program's file name, as rules match it.
    pub exe: String,
    /// What starts it (its file, or a packaged app's `shell:AppsFolder\…`).
    pub open: String,
    /// Windows' number of the monitor ("Monitor 1"), and the workspace (1-based).
    pub monitor: u32,
    pub workspace: u32,
    #[serde(skip_serializing_if = "is_false")]
    pub floating: bool,
}

/// `[dropdown]` (DECISIONS D62): the terminal that drops down from the top of the monitor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dropdown {
    /// The program and its arguments; empty for Windows Terminal (else PowerShell).
    pub command: String,
    /// How much of the monitor's height it takes, in percent.
    pub height: u32,
}

impl Default for Dropdown {
    fn default() -> Self {
        Dropdown {
            command: String::new(),
            height: 40,
        }
    }
}

pub const DROPDOWN_HEIGHT_RANGE: (u32, u32) = (20, 90);

/// `[gestures]` (DECISIONS D68, `kabuk_core::gesture`): swipes on a precision touchpad.
/// Off by default, as Windows has its own three- and four-finger gestures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gestures {
    pub enabled: bool,
    /// How many fingers swipe (3 or 4).
    pub fingers: u32,
    pub swipe_left: GestureAction,
    pub swipe_right: GestureAction,
    pub swipe_up: GestureAction,
    pub swipe_down: GestureAction,
}

impl Default for Gestures {
    fn default() -> Self {
        // As macOS: the fingers push the workspaces aside, up shows every workspace.
        Gestures {
            enabled: false,
            fingers: 3,
            swipe_left: GestureAction::NextWorkspace,
            swipe_right: GestureAction::PreviousWorkspace,
            swipe_up: GestureAction::Overview,
            swipe_down: GestureAction::AppGrid,
        }
    }
}

pub const GESTURE_FINGERS_RANGE: (u32, u32) = (3, 4);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureAction {
    #[default]
    None,
    /// The workspace after the shown one on the monitor in front (none after the last).
    NextWorkspace,
    PreviousWorkspace,
    /// Every workspace side by side (Win+Tab).
    Overview,
    /// Every app on a grid (SPEC 2.5b).
    AppGrid,
    Launcher,
}

impl GestureAction {
    pub const ALL: [GestureAction; 6] = [
        GestureAction::None,
        GestureAction::NextWorkspace,
        GestureAction::PreviousWorkspace,
        GestureAction::Overview,
        GestureAction::AppGrid,
        GestureAction::Launcher,
    ];
}

/// `[hot_corners]` (asked for by the user, `kabuk_core::hot_corner`): what the mouse
/// pushed into each corner of the screen does; nothing by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotCorners {
    pub top_left: HotCornerAction,
    pub top_right: HotCornerAction,
    pub bottom_left: HotCornerAction,
    pub bottom_right: HotCornerAction,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotCornerAction {
    #[default]
    None,
    /// Every app on a grid (SPEC 2.5b).
    AppGrid,
    /// Every workspace side by side (Win+Tab).
    Overview,
    Launcher,
    /// The screen saver starts (the displays turn off when none is chosen).
    ScreenSaver,
}

impl HotCornerAction {
    pub const ALL: [HotCornerAction; 5] = [
        HotCornerAction::None,
        HotCornerAction::AppGrid,
        HotCornerAction::Overview,
        HotCornerAction::Launcher,
        HotCornerAction::ScreenSaver,
    ];
}

/// `[system_theme]` (SPEC §2.10, DECISIONS D42): Windows itself follows the look, layer by
/// layer, each off by default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemTheme {
    /// Windows' app and system mode follow Kabuk's mode (unless that follows Windows),
    /// and Windows' accent colour follows the look's.
    pub colors: bool,
}

/// `[notices]`: Kabuk's own notices (SPEC §2.6).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notices {
    /// Do not disturb (PLAN M19 U20): Kabuk's notices are held back, as in game mode, and
    /// counted in one summary when it is turned off; a notice that the shell is in danger
    /// still shows.
    pub quiet: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema: SCHEMA_VERSION,
            appearance: Appearance::default(),
            wallpaper: Wallpaper::default(),
            screensaver: Screensaver::default(),
            bar: Bar::default(),
            windows: Windows::default(),
            rules: Rules::default(),
            game: Game::default(),
            launcher: Launcher::default(),
            shortcuts: Shortcuts::default(),
            language: Language::default(),
            oled: Oled::default(),
            create: Create::default(),
            system_theme: SystemTheme::default(),
            hot_corners: HotCorners::default(),
            dropdown: Dropdown::default(),
            gestures: Gestures::default(),
            notices: Notices::default(),
            profiles: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- appearance

/// The look a configuration without one gets: [`DEFAULT_LOOK`]. Debug builds take the
/// look named by `KABUK_TEST_DEFAULT_LOOK` instead, so the integration tests written
/// against Ada (its top bar, its sizes) keep it; release builds never read it.
pub fn default_look() -> Look {
    #[cfg(debug_assertions)]
    if let Some(look) = std::env::var(TEST_DEFAULT_LOOK)
        .ok()
        .and_then(|v| serde_json::from_value::<Look>(serde_json::Value::String(v)).ok())
    {
        return look;
    }
    DEFAULT_LOOK
}

/// The environment variable a debug build takes its default look from (tests).
pub const TEST_DEFAULT_LOOK: &str = "KABUK_TEST_DEFAULT_LOOK";

/// The ten looks (SPEC §3). Serialized by their ASCII proper names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Look {
    Ada,
    Cizgi,
    Devre,
    Ray,
    Konsol,
    Rihtim,
    Etkinlik,
    /// A panel with a task list (asked for by the user, 2026-10-15).
    Pano,
    /// Swiss graphic design: bold grotesque, a hairline grid.
    Izgara,
    /// Editorial: a serif page with Roman numerals.
    Defter,
}

impl Look {
    pub const ALL: [Look; 10] = [
        Look::Ada,
        Look::Cizgi,
        Look::Devre,
        Look::Ray,
        Look::Konsol,
        Look::Rihtim,
        Look::Etkinlik,
        Look::Pano,
        Look::Izgara,
        Look::Defter,
    ];

    /// The look's code in the design page and in `themes/<code>.toml`.
    pub fn code(self) -> &'static str {
        match self {
            Look::Ada => "a",
            Look::Cizgi => "b",
            Look::Devre => "c",
            Look::Ray => "d",
            Look::Konsol => "e",
            Look::Rihtim => "f",
            Look::Etkinlik => "g",
            Look::Pano => "h",
            Look::Izgara => "i",
            Look::Defter => "j",
        }
    }

    /// The bar edge each look uses unless the user picks another (SPEC §2.1).
    pub fn default_bar_edge(self) -> Edge {
        match self {
            Look::Ray => Edge::Left,
            Look::Konsol | Look::Pano => Edge::Bottom,
            _ => Edge::Top,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dark,
    Light,
    /// Follow the Windows app theme.
    System,
    /// Light from 07:00 to 19:00, dark otherwise.
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    Small,
    Normal,
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Animations {
    Full,
    /// Opacity only.
    Reduced,
    Off,
}

/// `appearance.cursors` (DECISIONS D20, D41).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorChoice {
    /// The user's own cursors, untouched.
    Windows,
    /// Kabuk's set in the look's own style and colour while Kabuk runs.
    Kabuk,
    /// Kabuk's set in `cursor_style` and `cursor_color`, whatever the look.
    Custom,
}

/// `appearance.cursor_style` (D41): the shapes of the looks' sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    /// Ada's: a round arrow.
    Round,
    /// Çizgi's: a slim arrow with sharp corners.
    Slim,
    /// Devre's: sharp corners and an accent via near the tip.
    Circuit,
    /// Ray's: a thick outline.
    Bold,
    /// Konsol's: sharp corners and a block text cursor.
    Block,
    /// A cartoon set: fat bodies in a thick outline, hands, coloured badges (asked for by
    /// the user with a picture); soft corners.
    Cartoon,
    /// The cartoon set with pointed corners.
    Comic,
}

impl CursorStyle {
    pub const ALL: [CursorStyle; 7] = [
        CursorStyle::Round,
        CursorStyle::Slim,
        CursorStyle::Circuit,
        CursorStyle::Bold,
        CursorStyle::Block,
        CursorStyle::Cartoon,
        CursorStyle::Comic,
    ];

    /// The look's own style.
    pub fn of(look: Look) -> CursorStyle {
        match look {
            Look::Ada | Look::Rihtim | Look::Etkinlik | Look::Pano => CursorStyle::Round,
            Look::Cizgi | Look::Defter => CursorStyle::Slim,
            Look::Izgara => CursorStyle::Block,
            Look::Devre => CursorStyle::Circuit,
            Look::Ray => CursorStyle::Bold,
            Look::Konsol => CursorStyle::Block,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            CursorStyle::Round => "round",
            CursorStyle::Slim => "slim",
            CursorStyle::Circuit => "circuit",
            CursorStyle::Bold => "bold",
            CursorStyle::Block => "block",
            CursorStyle::Cartoon => "cartoon",
            CursorStyle::Comic => "comic",
        }
    }
}

/// `appearance.cursor_color` (D41): the body's colour; the outline is the other one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorColor {
    Light,
    Dark,
    Accent,
}

impl CursorColor {
    pub const ALL: [CursorColor; 3] = [CursorColor::Light, CursorColor::Dark, CursorColor::Accent];

    /// The look's own colour: dark on light in Rıhtım and Etkinlik (macOS, GNOME).
    pub fn of(look: Look) -> CursorColor {
        match look {
            Look::Rihtim | Look::Etkinlik => CursorColor::Dark,
            _ => CursorColor::Light,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            CursorColor::Light => "light",
            CursorColor::Dark => "dark",
            CursorColor::Accent => "accent",
        }
    }
}

/// `appearance.cursor_size` (D41), for Kabuk's sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorSize {
    Normal,
    Large,
    Huge,
}

impl CursorSize {
    /// The size Windows loads cursors at (`CursorBaseSize`, pixels at 100 % scale), as
    /// its own pointer size slider writes it: 32 is Windows' normal size.
    pub fn base(self) -> u32 {
        match self {
            CursorSize::Normal => 32,
            CursorSize::Large => 48,
            CursorSize::Huge => 64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub look: Look,
    /// `"theme"` for the look's own accent, or a `#rrggbb` colour.
    pub accent: String,
    pub mode: Mode,
    pub text_size: TextSize,
    pub animations: Animations,
    pub cursors: CursorChoice,
    pub cursor_style: CursorStyle,
    pub cursor_color: CursorColor,
    pub cursor_size: CursorSize,
}

impl Appearance {
    /// The style and colour of Kabuk's set: the look's own, or the user's pick.
    pub fn cursor_pick(&self) -> (CursorStyle, CursorColor) {
        match self.cursors {
            CursorChoice::Custom => (self.cursor_style, self.cursor_color),
            _ => (CursorStyle::of(self.look), CursorColor::of(self.look)),
        }
    }
}

/// `accent` value meaning "use the look's own accent colour".
pub const THEME_ACCENT: &str = "theme";

/// The accent swatches offered in Settings (SPEC §2.9).
pub const ACCENT_SWATCHES: [&str; 7] = [
    "#8fd3ff", "#fbc419", "#7fe0b5", "#ff8f6b", "#b9a7ff", "#ff7eb6", "#ecebe7",
];

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            look: default_look(),
            accent: THEME_ACCENT.to_string(),
            mode: Mode::Dark,
            text_size: TextSize::Normal,
            animations: Animations::Full,
            cursors: CursorChoice::Windows,
            cursor_style: CursorStyle::Round,
            cursor_color: CursorColor::Light,
            cursor_size: CursorSize::Normal,
        }
    }
}

// ---------------------------------------------------------------- wallpaper

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperKind {
    /// Windows' own wallpaper, left as the user set it (default; the user's choice on
    /// 2026-10-13: Kabuk changes it only when another is chosen in Settings).
    Windows,
    /// Pure black.
    Black,
    /// Night gradient derived from the accent.
    Gradient,
    /// The user's image (`wallpaper.image`).
    Image,
    /// Animated circuit eye (same scene as the screen saver).
    Circuit,
    /// Animated constellation.
    Constellation,
    /// A wallpaper made in the Create tab (`wallpaper.generated`).
    Generated,
    /// A design of Kabuk's gallery (`wallpaper.gallery`, DECISIONS D58): a live one is
    /// played, a still one drawn once as the picture.
    Gallery,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wallpaper {
    /// Applies to every monitor without an entry in `monitor`.
    pub kind: WallpaperKind,
    /// Image path for `kind = "image"`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
    /// Name of the created wallpaper for `kind = "generated"`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub generated: String,
    /// The gallery design for `kind = "gallery"` (`wall-live/aurora-shore`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub gallery: String,
    /// Peak brightness, percent (10-100).
    pub brightness: u32,
    /// Pause animation when the desktop is covered, in full screen, in game mode and on battery.
    pub pause_when_hidden: bool,
    /// A still picture of the user's drifts by two pixels, a step every `drift_minutes`
    /// (OLED, DECISIONS D47).
    pub drift: bool,
    pub drift_minutes: u32,
    /// Per-monitor choices; monitors are numbered as Windows numbers them.
    #[serde(rename = "monitor", skip_serializing_if = "Vec::is_empty")]
    pub monitors: Vec<MonitorWallpaper>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorWallpaper {
    pub index: u32,
    pub kind: WallpaperKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub generated: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gallery: String,
}

pub const WALLPAPER_BRIGHTNESS_RANGE: (u32, u32) = (10, 100);
pub const WALLPAPER_DRIFT_MINUTES_RANGE: (u32, u32) = (1, 120);

impl Wallpaper {
    /// The choice for monitor `number` (Windows' numbering): its own entry, or the default
    /// (kind and image).
    pub fn for_monitor(&self, number: u32) -> (WallpaperKind, String) {
        match self.monitors.iter().find(|m| m.index == number) {
            Some(m) => (m.kind, m.image.clone()),
            None => (self.kind, self.image.clone()),
        }
    }

    /// The created wallpaper's id for monitor `number` (its own entry, or the default).
    pub fn generated_for(&self, number: u32) -> String {
        match self.monitors.iter().find(|m| m.index == number) {
            Some(m) => m.generated.clone(),
            None => self.generated.clone(),
        }
    }

    /// The gallery design for monitor `number` (its own entry, or the default).
    pub fn gallery_for(&self, number: u32) -> String {
        match self.monitors.iter().find(|m| m.index == number) {
            Some(m) => m.gallery.clone(),
            None => self.gallery.clone(),
        }
    }

    /// Whether monitor `number`'s wallpaper is played behind the icons: an animated kind,
    /// or a live design of the gallery.
    pub fn animated_for(&self, number: u32) -> bool {
        let (kind, _) = self.for_monitor(number);
        kind.animated() || (kind == WallpaperKind::Gallery && is_live_design(&self.gallery_for(number)))
    }
}

/// A gallery design that is played: its category ends in `-live` (`wall-live/…`).
pub fn is_live_design(key: &str) -> bool {
    key.split_once('/')
        .is_some_and(|(category, _)| category.ends_with("-live"))
}

impl WallpaperKind {
    /// Drawn by Kabuk's scene player behind the icons (circuit eye, constellation, a
    /// created wallpaper).
    pub fn animated(self) -> bool {
        matches!(
            self,
            WallpaperKind::Circuit | WallpaperKind::Constellation | WallpaperKind::Generated
        )
    }
}

impl Default for Wallpaper {
    fn default() -> Self {
        Wallpaper {
            kind: WallpaperKind::Windows,
            image: String::new(),
            generated: String::new(),
            gallery: String::new(),
            brightness: 60,
            pause_when_hidden: true,
            drift: true,
            drift_minutes: 10,
            monitors: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- screen saver

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaverKind {
    /// Milvasoft circuit board eye.
    Circuit,
    Constellation,
    /// A design of Kabuk's gallery (`screensaver.gallery`, DECISIONS D58).
    Gallery,
    /// No saver; turn the display off.
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Screensaver {
    pub kind: SaverKind,
    /// The gallery design for `kind = "gallery"` (`saver-live/comet-calligraphy`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub gallery: String,
    /// One of [`SAVER_WAIT_CHOICES`].
    pub wait_minutes: u32,
    /// Honour display-on requests (do not start while a video plays).
    pub respect_video: bool,
}

pub const SAVER_WAIT_CHOICES: [u32; 4] = [3, 5, 10, 15];

impl Default for Screensaver {
    fn default() -> Self {
        Screensaver {
            kind: SaverKind::Circuit,
            gallery: String::new(),
            wait_minutes: 5,
            respect_video: true,
        }
    }
}

// ---------------------------------------------------------------- bar

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// `bar.position`: the look's default edge or a fixed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    Default,
    Top,
    Bottom,
    Left,
    Right,
}

impl BarPosition {
    pub fn edge(self, look: Look) -> Edge {
        match self {
            BarPosition::Default => look.default_bar_edge(),
            BarPosition::Top => Edge::Top,
            BarPosition::Bottom => Edge::Bottom,
            BarPosition::Left => Edge::Left,
            BarPosition::Right => Edge::Right,
        }
    }
}

/// `bar.background`: what fills the bar's edge over the wallpaper (DECISIONS D28).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarBackground {
    /// The desk colour, as the design shows the bars on its black desk.
    Solid,
    /// The wallpaper blurred and tinted with the desk colour, as Windows' own taskbar.
    Blur,
    /// The bar sits on the wallpaper.
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarMonitors {
    All,
    Main,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bar {
    pub position: BarPosition,
    pub monitors: BarMonitors,
    pub autohide: bool,
    /// What fills the bar's whole reserved edge over the wallpaper (DECISIONS D28).
    pub background: BarBackground,
    /// The bar's thickness in DIPs (its height on the top or bottom edge, its width on
    /// the left or right); `None` keeps the look's own (DECISIONS D29).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Tray icons kept on the bar; the rest open from the overflow button (DECISIONS D26).
    pub tray_shown: u32,
    pub modules: Modules,
    /// The bar strip's colour, `#RRGGBB`; empty keeps the look's own (DECISIONS D57).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub color: String,
}

pub const TRAY_SHOWN_RANGE: (u32, u32) = (0, 12);
pub const BAR_HEIGHT_RANGE: (u32, u32) = (24, 72);

impl Default for Bar {
    fn default() -> Self {
        Bar {
            position: BarPosition::Default,
            monitors: BarMonitors::All,
            autohide: false,
            background: BarBackground::Solid,
            height: None,
            tray_shown: 3,
            modules: Modules::default(),
            color: String::new(),
        }
    }
}

/// Bar modules (SPEC §2.1). The workspace indicator is always shown and is not listed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Modules {
    pub title: bool,
    /// Pinned and open apps (DECISIONS D25).
    pub taskbar: bool,
    pub clock: bool,
    pub date: bool,
    pub media: bool,
    pub cpu: bool,
    pub ram: bool,
    pub gpu: bool,
    pub net: bool,
    pub vol: bool,
    pub tray: bool,
    pub notif: bool,
    pub power: bool,
    /// The button that opens the grid of every app (DECISIONS D56).
    pub app_grid: bool,
    /// Windows' network indicator: Wi-Fi signal, Ethernet or none (DECISIONS D88).
    pub network: bool,
    /// The battery's charge, on a computer with one (D88).
    pub battery: bool,
    /// The input language, with more than one installed (D88).
    pub keyboard: bool,
    /// An icon while a program uses the microphone, the camera or the location (D88).
    pub privacy: bool,
    /// A thin corner at the bar's end: every window down, a second click brings them back
    /// (D88).
    pub show_desktop: bool,
    /// A click on the clock or the bell opens Kabuk's calendar and notifications (D88);
    /// off, Windows' notification centre.
    pub calendar: bool,
    /// The quick settings button: Wi-Fi, Bluetooth, airplane mode, brightness, volume
    /// (D88).
    pub quick_settings: bool,
}

impl Default for Modules {
    fn default() -> Self {
        Modules {
            title: true,
            taskbar: true,
            clock: true,
            date: true,
            media: true,
            cpu: true,
            ram: true,
            gpu: true,
            net: true,
            vol: true,
            tray: true,
            notif: true,
            power: true,
            app_grid: true,
            network: true,
            battery: true,
            keyboard: true,
            privacy: true,
            show_desktop: true,
            calendar: true,
            quick_settings: true,
        }
    }
}

// ---------------------------------------------------------------- windows

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceLayout {
    Tiling,
    Floating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TilingLayout {
    Dwindle,
    Master,
}

/// `windows.minimize_effect` (asked for by the user on 2026-10-14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MinimizeEffect {
    /// macOS's: the window pours into its icon on the bar and back out.
    Genie,
    /// Windows' own transition.
    Windows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewWindow {
    Right,
    Below,
    Floating,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Windows {
    /// Workspaces per monitor (1-9; shortcuts reach 1-9).
    pub workspaces: u32,
    /// Workspaces always shown on the bar; occupied and active ones are shown too.
    pub shown_workspaces: u32,
    pub default_layout: WorkspaceLayout,
    pub tiling_layout: TilingLayout,
    /// Gap in px (0-32). Absent means the look's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<u32>,
    pub new_window: NewWindow,
    pub drag_to_float: bool,
    pub alt_drag: bool,
    /// Double-clicking a floating window's title tiles it again.
    pub double_click_to_tile: bool,
    pub active_border: bool,
    /// Other programs' title bars and corners follow the look (DECISIONS D30).
    pub follow_look: bool,
    /// How managed windows minimise and restore (with animations on in full).
    pub minimize_effect: MinimizeEffect,
    /// The workspaces' names by number, from workspace 1 (PLAN M19 U20); a missing or
    /// empty one keeps its number. Read through [`Windows::workspace_name`].
    pub workspace_names: Vec<String>,
}

pub const WORKSPACE_RANGE: (u32, u32) = (1, 9);
/// The longest a workspace's name is shown (characters); a longer one is cut.
pub const WORKSPACE_NAME_MAX: usize = 24;

impl Windows {
    /// Workspace `n`'s name (1-based), trimmed and at most [`WORKSPACE_NAME_MAX`]
    /// characters; `None` when it has none.
    pub fn workspace_name(&self, n: u32) -> Option<String> {
        let i = usize::try_from(n.checked_sub(1)?).ok()?;
        let name = self.workspace_names.get(i)?.trim();
        (!name.is_empty()).then(|| name.chars().take(WORKSPACE_NAME_MAX).collect())
    }
}
pub const GAP_RANGE: (u32, u32) = (0, 32);

impl Default for Windows {
    fn default() -> Self {
        Windows {
            workspaces: 9,
            shown_workspaces: 4,
            default_layout: WorkspaceLayout::Tiling,
            tiling_layout: TilingLayout::Dwindle,
            gap: None,
            new_window: NewWindow::Right,
            drag_to_float: true,
            alt_drag: true,
            double_click_to_tile: true,
            active_border: true,
            follow_look: true,
            minimize_effect: MinimizeEffect::Genie,
            workspace_names: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- rules

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    /// Float windows whose minimum size does not fit the layout.
    pub float_unfit: bool,
    /// Built-in rules: dialogs float centred, splash screens ignored, Taskmgr.exe floats.
    pub defaults: bool,
    #[serde(rename = "rule", skip_serializing_if = "Vec::is_empty")]
    pub user: Vec<Rule>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            float_unfit: true,
            defaults: true,
            user: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

/// A user window rule (SPEC §2.3). Matchers that are set must all match.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exe: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Matchers are regular expressions instead of plain (case-insensitive) text.
    #[serde(skip_serializing_if = "is_false")]
    pub regex: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub float: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub ignore: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// Alt + clicks stay with the program: Kabuk never grabs its windows (DECISIONS D23).
    #[serde(skip_serializing_if = "is_false")]
    pub no_grab: bool,
}

pub const MONITOR_RANGE: (u32, u32) = (1, 16);
pub const MIN_WIDTH_RANGE: (u32, u32) = (1, 16384);

fn is_false(b: &bool) -> bool {
    !*b
}

// ---------------------------------------------------------------- game mode

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Game {
    pub detect: bool,
    pub hide_bar: bool,
    pub pause_tiling: bool,
    pub silence_notifications: bool,
    pub other_monitors_normal: bool,
}

impl Default for Game {
    fn default() -> Self {
        Game {
            detect: true,
            hide_bar: true,
            pause_tiling: true,
            silence_notifications: true,
            other_monitors_normal: true,
        }
    }
}

// ---------------------------------------------------------------- launcher

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Launcher {
    pub sources: LauncherSources,
    /// Folders searched for files; `%VAR%` environment references are expanded.
    pub indexed_folders: Vec<String>,
}

impl Default for Launcher {
    fn default() -> Self {
        Launcher {
            sources: LauncherSources::default(),
            indexed_folders: vec!["%USERPROFILE%\\Documents".to_string()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSources {
    pub windows: bool,
    pub apps: bool,
    pub recent: bool,
    pub files: bool,
    pub bookmarks: bool,
    pub commands: bool,
    pub calculator: bool,
}

impl Default for LauncherSources {
    fn default() -> Self {
        LauncherSources {
            windows: true,
            apps: true,
            recent: true,
            files: true,
            bookmarks: true,
            commands: true,
            calculator: true,
        }
    }
}

// ---------------------------------------------------------------- shortcuts

/// Shortcuts (SPEC §6). Each action takes a list of chords. The emergency chord
/// `Ctrl+Alt+Shift+End` is fixed and not part of the schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub launcher: Vec<String>,
    /// Modifiers + 1..9 switch workspace.
    pub workspace: String,
    /// Modifiers + 1..9 move the focused window to a workspace.
    pub move_to_workspace: String,
    pub focus_left: Vec<String>,
    pub focus_down: Vec<String>,
    pub focus_up: Vec<String>,
    pub focus_right: Vec<String>,
    pub move_left: Vec<String>,
    pub move_down: Vec<String>,
    pub move_up: Vec<String>,
    pub move_right: Vec<String>,
    pub toggle_float: Vec<String>,
    pub close_window: Vec<String>,
    pub cycle_layout: Vec<String>,
    /// Shows or hides the hidden workspace on the focused monitor (DECISIONS D34).
    pub toggle_scratchpad: Vec<String>,
    /// Sends the focused window to the hidden workspace.
    pub to_scratchpad: Vec<String>,
    /// Makes the focused window a tabbed group, or breaks its group up (D34).
    pub toggle_group: Vec<String>,
    /// Shows the next tab of the focused group.
    pub next_tab: Vec<String>,
    /// Opens the overview of every workspace (D34).
    pub overview: Vec<String>,
    /// Starts the screen saver now (the display turns off when none is chosen).
    pub start_saver: Vec<String>,
    /// Opens the grid of every app, as macOS's Launchpad (DECISIONS D56).
    pub app_grid: Vec<String>,
    /// Shows the card of Kabuk's main shortcuts (DECISIONS D61).
    pub keys_card: Vec<String>,
    /// Drops the terminal down from the top of the monitor, or hides it (D62).
    pub dropdown: Vec<String>,
    /// Held while dragging: left button moves, right button resizes.
    pub drag: String,
}

fn chords(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

impl Default for Shortcuts {
    /// On the Win key (DECISIONS D40), clear of Windows' own that people use (Win+L,
    /// Win+E, Win+V, Win+Shift+S) and of the input-language switch (Win+Space, SPEC §6).
    fn default() -> Self {
        Shortcuts {
            launcher: chords(&["Win+R"]),
            workspace: "Win".into(),
            move_to_workspace: "Win+Shift".into(),
            focus_left: chords(&["Win+Left"]),
            focus_down: chords(&["Win+Down"]),
            focus_up: chords(&["Win+Up"]),
            focus_right: chords(&["Win+Right"]),
            move_left: chords(&["Win+Shift+Left"]),
            move_down: chords(&["Win+Shift+Down"]),
            move_up: chords(&["Win+Shift+Up"]),
            move_right: chords(&["Win+Shift+Right"]),
            toggle_float: chords(&["Win+F"]),
            close_window: chords(&["Win+Shift+Q"]),
            cycle_layout: chords(&["Win+Y"]),
            toggle_scratchpad: chords(&["Win+Z"]),
            to_scratchpad: chords(&["Win+Shift+Z"]),
            toggle_group: chords(&["Win+T"]),
            next_tab: chords(&["Win+Shift+T"]),
            overview: chords(&["Win+Tab"]),
            start_saver: chords(&["Win+Shift+L"]),
            app_grid: chords(&["Win+Shift+A"]),
            keys_card: chords(&["Win+F1"]),
            dropdown: chords(&["Win+Enter"]),
            drag: "Alt".into(),
        }
    }
}

// ---------------------------------------------------------------- language

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageChoice {
    System,
    Tr,
    En,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockFormat {
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "12h")]
    H12,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Language {
    pub language: LanguageChoice,
    pub clock: ClockFormat,
}

impl Default for Language {
    fn default() -> Self {
        Language {
            language: LanguageChoice::System,
            clock: ClockFormat::H24,
        }
    }
}

// ---------------------------------------------------------------- OLED care

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Oled {
    pub bar_shift: bool,
    pub idle_dim: bool,
    /// Seconds without input before dimming (10 s - 60 min).
    pub dim_after_secs: u32,
}

pub const DIM_AFTER_RANGE: (u32, u32) = (10, 3600);

impl Default for Oled {
    fn default() -> Self {
        Oled {
            bar_shift: true,
            idle_dim: true,
            dim_after_secs: 60,
        }
    }
}

// ---------------------------------------------------------------- create (wallpaper generation)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Openai,
    Ollama,
}

/// Wallpaper generation. API keys live in Windows Credential Manager, never here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Create {
    pub provider: Provider,
    pub ollama_url: String,
}

impl Default for Create {
    fn default() -> Self {
        Create {
            provider: Provider::Claude,
            ollama_url: "http://localhost:11434".into(),
        }
    }
}
