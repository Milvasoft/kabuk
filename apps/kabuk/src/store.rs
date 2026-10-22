//! Kabuk from the Microsoft Store (DECISIONS D89): its package keeps its writes to the
//! registry and to AppData to itself (the Store refused the capability that lifts this),
//! so what reaches Windows only through them is not offered: the look's cursors, its
//! screen saver and Windows' colours following it.

/// Integration tests: run as the Store's Kabuk would (set in the test's environment).
#[cfg(feature = "integration")]
pub const PRETEND_ENV: &str = "KABUK_DEBUG_AS_STORE";

/// Whether this is the Store's Kabuk.
pub fn store_version() -> bool {
    #[cfg(feature = "integration")]
    if std::env::var_os(PRETEND_ENV).is_some() {
        return true;
    }
    kabuk_win::package::is_packaged()
}
