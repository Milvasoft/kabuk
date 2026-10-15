//! The app grid's measures (DIPs unless said otherwise), after macOS's Launchpad: large
//! icons with their names under them on the blurred, darkened wallpaper, a search field
//! at the top and the page dots at the bottom. Every look shares them; the colours are the
//! look's own (white text over the darkened picture).

use kabuk_ui::geom::Size;

/// An app's icon.
pub const ICON: f32 = 88.0;
/// The tile drawn until an app's own picture has loaded, and its corners.
pub const PLACEHOLDER_GLYPH: f32 = 40.0;
pub const PLACEHOLDER_RADIUS: f32 = 20.0;
/// An app's name under its icon: its size, the space above it and its widest.
pub const LABEL: f32 = 13.0;
pub const LABEL_GAP: f32 = 10.0;
pub const LABEL_WIDTH: f32 = 150.0;
/// The keyboard's selection around an icon and its name, and its corners.
pub const SELECTION_PAD: f32 = 12.0;
pub const SELECTION_RADIUS: f32 = 18.0;

/// The smallest cell an app takes; cells share out the room left between the margins.
pub const CELL: Size = Size { w: 180.0, h: 150.0 };
/// The fewest and most columns and rows of a page (macOS's Launchpad: 7 × 5).
pub const COLUMNS: (usize, usize) = (4, 7);
pub const ROWS: (usize, usize) = (2, 5);
/// The margins: at the sides, above the cells (the search field's room) and below them
/// (the dots' room).
pub const SIDE: f32 = 120.0;
pub const TOP: f32 = 120.0;
pub const BOTTOM: f32 = 100.0;

/// The search field: its size, how far below the top edge it is, its corners, its icon
/// and text sizes and its side padding.
pub const SEARCH: Size = Size { w: 280.0, h: 34.0 };
pub const SEARCH_TOP: f32 = 44.0;
pub const SEARCH_RADIUS: f32 = 10.0;
pub const SEARCH_ICON: f32 = 15.0;
pub const SEARCH_TEXT: f32 = 14.0;
pub const SEARCH_PAD: f32 = 12.0;
pub const SEARCH_GAP: f32 = 8.0;
/// Once it takes the keys: its frame in the accent colour, this wide, and a caret this
/// wide, as tall as a line of its text (the launcher's).
pub const SEARCH_ACTIVE_BORDER: f32 = 1.5;
pub const CARET: f32 = 1.5;
pub const CARET_LINE: f32 = 1.25;

/// Editing the grid (moving and hiding apps): a press held this long on an app begins it
/// (milliseconds), as a right click does; each app shows a badge this big that hides it,
/// over its icon's top-right corner (asked for by the user) by this share of the badge; the dragged app follows
/// the mouse; held this close to the side edges (DIPs) for this long (milliseconds), it
/// turns the page. Done stands where the search field is, this wide, with the words that
/// tell what to do under it.
pub const HOLD_MS: u64 = 500;
/// While editing, each icon wiggles as on an iPhone (asked for by the user): this many
/// degrees each way, once per this many seconds, drawn this often (milliseconds).
pub const WIGGLE_DEGREES: f32 = 2.5;
pub const WIGGLE_PERIOD: f32 = 0.26;
pub const WIGGLE_FRAME_MS: u64 = 33;
/// Apps making room for a dragged one, or taking their new places, slide there this long
/// (seconds; asked for by the user).
pub const REFLOW_SECONDS: f32 = 0.2;
pub const BADGE: f32 = 24.0;
pub const BADGE_OVER: f32 = 0.3;
pub const BADGE_TEXT: f32 = 16.0;
pub const EDGE_TURN: f32 = 60.0;
pub const EDGE_TURN_MS: u64 = 600;
pub const DONE_WIDTH: f32 = 120.0;
pub const EDIT_HINT_GAP: f32 = 10.0;
pub const EDIT_HINT_TEXT: f32 = 13.0;

/// The page dots: their size, the space between them, the room around each that a
/// click finds, and how far above the bottom edge they are.
pub const DOT: f32 = 8.0;
pub const DOT_GAP: f32 = 4.0;
pub const DOT_HIT: f32 = 6.0;
pub const DOTS_BOTTOM: f32 = 44.0;

/// The pages whose icons are loaded ahead, this long after Kabuk starts.
pub const PAGES_AHEAD: usize = 2;
pub const AHEAD_AFTER_SECONDS: u64 = 5;

/// The words shown when nothing matches the search.
pub const EMPTY_TEXT: f32 = 18.0;

/// A drag turns the page once it went past this share of the page's width.
pub const SWIPE_SHARE: f32 = 0.15;
/// A press becomes a drag once the mouse moved this far (DIPs).
pub const DRAG_START: f32 = 8.0;

/// The wallpaper under the grid: read this many times smaller than the monitor, blurred
/// with a box this many pixels each side that many times (about a Gaussian), and darkened
/// to this share of its light so white text reads on any picture.
pub const BLUR_SHRINK: u32 = 16;
pub const BLUR_RADIUS: usize = 2;
pub const BLUR_PASSES: usize = 3;
pub const BLUR_DIM: f32 = 0.6;
/// The screen itself, taken as the grid opens, is the background first (the wallpaper
/// only when it cannot be taken): darker than the wallpaper, as windows can be white.
/// How long the bar thread waits for it (milliseconds).
pub const SCREEN_DIM: f32 = 0.45;
pub const SCREEN_WAIT_MS: u64 = 150;
