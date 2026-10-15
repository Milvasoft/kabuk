//! Wallpaper generation (SPEC §2.8, §9; ARCHITECTURE §11b): the wallpaper format, what
//! the model is told, its reply, the providers' requests and answers, the OLED checks
//! and the three-try loop. Pure: the network, the credential store and the GPU live
//! elsewhere (`kabuk-win`, `kabuk-ui`).

pub mod check;
pub mod format;
pub mod gallery;
pub mod pipeline;
pub mod prompt;
pub mod providers;
pub mod reply;
pub mod store;
