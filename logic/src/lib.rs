//! Pure, hardware-free calendar/timezone arithmetic and prayer-time
//! parsing/formatting logic for namaz-vakti.
//!
//! This crate deliberately has no ESP-IDF or hardware dependencies so it can
//! be built and unit tested with a plain, stable host Rust toolchain — see
//! the repo's `logic/rust-toolchain.toml` and the `Tests` job in
//! `.github/workflows/rust.yml`. The main firmware crate (`namaz-vakti`)
//! depends on this crate via a path dependency and re-exports the bits it
//! needs from `src/time_utils.rs` and `src/prayer.rs`.

pub mod cloud;
pub mod csr;
pub mod language;
pub mod location;
pub mod prayer_times;
pub mod secure_element;
pub mod time_utils;
pub mod touch_calibration;
pub mod wifi_credentials;
pub mod zone;
