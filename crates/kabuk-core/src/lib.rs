//! Platform-independent core of Kabuk: configuration, persisted state, clock,
//! restart policy, instance naming and command lines. No Windows dependencies.

pub mod app_grid;
pub mod cli;
pub mod clock;
pub mod config;
pub mod fsutil;
pub mod fullscreen;
pub mod genie;
pub mod grab;
pub mod hang;
pub mod install;
pub mod instance;
pub mod ipc;
pub mod jumplist;
pub mod logging;
pub mod oled;
pub mod paths;
pub mod restart;
pub mod rules;
pub mod state;
pub mod tray;
pub mod update;
pub mod wallpaper;
pub mod wm;
