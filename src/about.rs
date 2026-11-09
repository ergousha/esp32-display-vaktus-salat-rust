//! The About page (issue #21): shows fixed hardware identity, the firmware
//! version from the build metadata, and the station MAC address, returning to
//! Settings via the shared top-right back-arrow tap square.
//!
//! Same draw-once-then-poll interaction model as the other sub-screens. The
//! caller ([`crate::main`]) resolves the MAC from the active network interface
//! and passes it in, so this module stays free of WiFi/netif handles.

use std::thread::sleep;
use std::time::Duration;

use embedded_graphics::prelude::*;
use embedded_hal::spi::SpiDevice;

use namaz_vakti_logic::language::{self, Language, Msg};

use crate::text::{self, HAlign};
use crate::touch::Xpt2046;
use crate::touch_calibration::Calibration;
use crate::{keyboard, settings_screen, Rgb565};

/// Fixed hardware model string for this board. Not localized — it's a product
/// identifier, shown verbatim in every language.
const HARDWARE_MODEL: &str = "4.0inch ESP32-32E Display";

/// Firmware version, taken from the crate's build metadata (`Cargo.toml`).
const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

// Row layout: a localized label line, then its Latin value just below.
const CONTENT_X: i32 = 20;
const FIRST_ROW_Y: i32 = 70;
const ROW_PITCH: i32 = 62;

/// Renders the About page and blocks until the user taps the back arrow.
pub fn run<D, SPI>(
    display: &mut D,
    touch: &mut Xpt2046<SPI>,
    calibration: &Calibration,
    lang: Language,
    mac: &str,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    SPI: SpiDevice,
{
    draw(display, lang, mac)?;
    keyboard::wait_for_release(touch);

    let mut press_handled = false;
    loop {
        match touch.sample_position() {
            Ok(Some((xr, yr))) => {
                if !press_handled {
                    press_handled = true;
                    let (x, y) = calibration.to_screen(xr, yr);
                    if settings_screen::point_in_icon(x, y) {
                        return Ok(());
                    }
                }
            }
            Ok(None) => press_handled = false,
            Err(e) => log::warn!("Touch read failed in About: {e:?}"),
        }
        sleep(Duration::from_millis(40));
    }
}

fn draw<D>(display: &mut D, lang: Language, mac: &str) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    display
        .clear(crate::col_bg())
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    settings_screen::draw_back_icon(display)?;

    text::draw_line(
        display,
        language::text(lang, Msg::AboutTitle),
        Point::new(240, 24),
        HAlign::Center,
        crate::col_text(),
        lang,
        text::Size::Medium,
    )?;

    draw_field(display, lang, 0, Msg::AboutHardware, HARDWARE_MODEL)?;
    draw_field(display, lang, 1, Msg::AboutFirmware, FIRMWARE_VERSION)?;
    draw_field(display, lang, 2, Msg::AboutMac, mac)?;
    // Not localized: thing name, "awaiting registration (MAC)", errors. Cut to
    // what fits the 480 px line in the Medium font.
    let cloud: String = crate::cloud::status_line().chars().take(44).collect();
    draw_field(display, lang, 3, Msg::AboutCloud, &cloud)?;
    Ok(())
}

/// Draws one labelled field: the localized label, with the value (model /
/// version / MAC) left-aligned under it.
fn draw_field<D>(
    display: &mut D,
    lang: Language,
    row: i32,
    label: Msg,
    value: &str,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = FIRST_ROW_Y + row * ROW_PITCH;

    text::draw_line(
        display,
        language::text(lang, label),
        Point::new(CONTENT_X, y),
        HAlign::Left,
        crate::col_dim(),
        lang,
        text::Size::Small,
    )?;
    text::draw_line(
        display,
        value,
        Point::new(CONTENT_X, y + 26),
        HAlign::Left,
        crate::col_text(),
        Language::English,
        text::Size::Medium,
    )?;
    Ok(())
}
