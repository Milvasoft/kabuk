//! Platform-independent core of Kabuk: configuration, persisted state, clock,
//! restart policy, instance naming and command lines. No Windows dependencies.

pub mod app_grid;
pub mod battery;
pub mod calendar;
pub mod cli;
pub mod clock;
pub mod config;
pub mod dropdown;
pub mod fsutil;
pub mod fullscreen;
pub mod genie;
pub mod gesture;
pub mod grab;
pub mod hang;
pub mod hot_corner;
pub mod input_lang;
pub mod install;
pub mod instance;
pub mod ipc;
pub mod jumplist;
pub mod logging;
pub mod network;
pub mod oled;
pub mod paths;
pub mod privacy;
pub mod profile;
pub mod restart;
pub mod rules;
pub mod state;
pub mod theme_file;
pub mod tray;
pub mod update;
pub mod wallpaper;
pub mod wm;
