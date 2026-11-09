//! Shared on-screen keyboard and single-line text-entry screen.
//!
//! Extracted from the WiFi setup flow (issue #11) so the same debounced,
//! draw-once-then-poll keyboard can back both the WiFi passphrase/SSID entry
//! and the location search boxes (issue #21). Geometry constants are shared
//! between drawing and hit-testing so a tap always maps back to exactly what
//! was drawn, and the header back-arrow tap square doubles as "cancel".
//!
//! Everything here is hardware-facing (it drives the panel and polls the touch
//! controller); callers own what the returned string is used for.

use std::thread::sleep;
use std::time::Duration;

use embedded_graphics::{
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
};
use embedded_hal::spi::SpiDevice;

use namaz_vakti_logic::language::{self, Language, Msg};

use crate::text::{self, HAlign};
use crate::touch::Xpt2046;
use crate::touch_calibration::Calibration;
use crate::{settings_screen, Rgb565};

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Lower,
    Upper,
    Symbols,
}

#[derive(Clone, Copy)]
enum KeyAction {
    Char(char),
    Shift,
    ToggleSymbols,
    Backspace,
    Space,
    Done,
}

struct PlacedKey {
    action: KeyAction,
    label: String,
    /// Language the label is written in — English for the character keys, the
    /// active language for the localized controls.
    label_lang: Language,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

const KB_TOP: i32 = 76;
const KEY_W: u32 = 44;
const KEY_H: u32 = 44;
const KEY_HGAP: i32 = 3;
const KEY_VGAP: i32 = 4;
const KB_MARGIN: i32 = 6;

fn key_pitch() -> i32 {
    KEY_H as i32 + KEY_VGAP
}

/// Builds the full laid-out key list for a layer, control row included. This is
/// the single source of truth shared by drawing and hit-testing.
fn layout_keys(layer: Layer, lang: Language) -> Vec<PlacedKey> {
    let rows: [&str; 4] = match layer {
        Layer::Lower => ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm"],
        Layer::Upper => ["1234567890", "QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"],
        Layer::Symbols => ["1234567890", "!@#$%^&*()", "-_=+[]{}\\|", ";:'\",.<>/?"],
    };

    let mut keys = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let n = row.chars().count() as i32;
        let row_w = n * KEY_W as i32 + (n - 1) * KEY_HGAP;
        let start_x = (480 - row_w) / 2;
        let y = KB_TOP + r as i32 * key_pitch();
        for (c, ch) in row.chars().enumerate() {
            keys.push(PlacedKey {
                action: KeyAction::Char(ch),
                label: ch.to_string(),
                label_lang: Language::English,
                x: start_x + c as i32 * (KEY_W as i32 + KEY_HGAP),
                y,
                w: KEY_W,
                h: KEY_H,
            });
        }
    }

    // Control row. A width of 0 marks the flex key (space), which absorbs the
    // leftover width so the row always spans edge-to-edge.
    let done = language::text(lang, Msg::KeyDone).to_string();
    let space = language::text(lang, Msg::KeySpace).to_string();
    let controls: Vec<(KeyAction, String, Language, u32)> = match layer {
        Layer::Symbols => vec![
            (
                KeyAction::ToggleSymbols,
                "ABC".into(),
                Language::English,
                58,
            ),
            (KeyAction::Space, space, lang, 0),
            (KeyAction::Backspace, "del".into(), Language::English, 58),
            (KeyAction::Done, done, lang, 64),
        ],
        _ => vec![
            (
                KeyAction::ToggleSymbols,
                "123".into(),
                Language::English,
                58,
            ),
            (KeyAction::Shift, "Aa".into(), Language::English, 58),
            (KeyAction::Space, space, lang, 0),
            (KeyAction::Backspace, "del".into(), Language::English, 58),
            (KeyAction::Done, done, lang, 64),
        ],
    };

    let avail = 480 - 2 * KB_MARGIN;
    let n = controls.len() as i32;
    let fixed: i32 = controls.iter().map(|(_, _, _, w)| *w as i32).sum();
    let flex = (avail - fixed - (n - 1) * KEY_HGAP).max(KEY_W as i32);
    let y = KB_TOP + 4 * key_pitch();
    let mut x = KB_MARGIN;
    for (action, label, label_lang, w) in controls {
        let w = if w == 0 { flex as u32 } else { w };
        keys.push(PlacedKey {
            action,
            label,
            label_lang,
            x,
            y,
            w,
            h: KEY_H,
        });
        x += w as i32 + KEY_HGAP;
    }
    keys
}

/// Runs a single text-entry screen. Returns the typed string on "done", or
/// `None` if the user tapped the back arrow.
///
/// `title` is the localized prompt shown above the field; `context` is an
/// optional Latin string shown right-aligned next to it (the WiFi SSID when
/// entering a passphrase, or the location breadcrumb when searching).
pub fn run_text_entry<D, SPI>(
    display: &mut D,
    touch: &mut Xpt2046<SPI>,
    calibration: &Calibration,
    lang: Language,
    title: Msg,
    context: Option<&str>,
    max_len: usize,
) -> anyhow::Result<Option<String>>
where
    D: DrawTarget<Color = Rgb565>,
    SPI: SpiDevice,
{
    let mut buf = String::new();
    let mut layer = Layer::Lower;

    draw_keyboard(display, lang, title, context, &buf, layer)?;
    wait_for_release(touch);

    let mut press_handled = false;
    loop {
        match touch.sample_position() {
            Ok(Some((xr, yr))) => {
                if !press_handled {
                    press_handled = true;
                    let (x, y) = calibration.to_screen(xr, yr);
                    if settings_screen::point_in_icon(x, y) {
                        return Ok(None);
                    }
                    if let Some(action) = hit_key(x, y, layer, lang) {
                        match action {
                            KeyAction::Char(c) => {
                                if buf.chars().count() < max_len {
                                    buf.push(c);
                                    draw_text_field(display, lang, title, context, &buf)?;
                                }
                            }
                            KeyAction::Space => {
                                if buf.chars().count() < max_len {
                                    buf.push(' ');
                                    draw_text_field(display, lang, title, context, &buf)?;
                                }
                            }
                            KeyAction::Backspace => {
                                buf.pop();
                                draw_text_field(display, lang, title, context, &buf)?;
                            }
                            KeyAction::Shift => {
                                layer = match layer {
                                    Layer::Lower => Layer::Upper,
                                    Layer::Upper => Layer::Lower,
                                    Layer::Symbols => Layer::Lower,
                                };
                                draw_keyboard(display, lang, title, context, &buf, layer)?;
                            }
                            KeyAction::ToggleSymbols => {
                                layer = if layer == Layer::Symbols {
                                    Layer::Lower
                                } else {
                                    Layer::Symbols
                                };
                                draw_keyboard(display, lang, title, context, &buf, layer)?;
                            }
                            KeyAction::Done => return Ok(Some(buf)),
                        }
                    }
                }
            }
            Ok(None) => press_handled = false,
            Err(e) => log::warn!("Touch read failed in keyboard: {e:?}"),
        }
        sleep(Duration::from_millis(40));
    }
}

/// Maps a calibrated tap to the key action under it, if any.
fn hit_key(x: i32, y: i32, layer: Layer, lang: Language) -> Option<KeyAction> {
    layout_keys(layer, lang)
        .into_iter()
        .find(|k| x >= k.x && x < k.x + k.w as i32 && y >= k.y && y < k.y + k.h as i32)
        .map(|k| k.action)
}

/// Full keyboard repaint: text field + every key for `layer`.
fn draw_keyboard<D>(
    display: &mut D,
    lang: Language,
    title: Msg,
    context: Option<&str>,
    buf: &str,
    layer: Layer,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    display
        .clear(crate::col_bg())
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    settings_screen::draw_back_icon(display)?;
    draw_field_contents(display, lang, title, context, buf)?;

    for key in layout_keys(layer, lang) {
        draw_key(display, &key)?;
    }
    Ok(())
}

/// Repaints just the title/field region (used on every keystroke so the keys
/// don't flicker).
fn draw_text_field<D>(
    display: &mut D,
    lang: Language,
    title: Msg,
    context: Option<&str>,
    buf: &str,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(0, 0), Size::new(480, (KB_TOP - 2) as u32))
        .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
            crate::col_bg(),
        ))
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    // The back icon lives in this band, so repaint it after the clear.
    settings_screen::draw_back_icon(display)?;
    draw_field_contents(display, lang, title, context, buf)
}

const FIELD_X: i32 = 8;
const FIELD_Y: i32 = 34;
const FIELD_W: u32 = 464;
const FIELD_H: u32 = 32;

fn draw_field_contents<D>(
    display: &mut D,
    lang: Language,
    title: Msg,
    context: Option<&str>,
    buf: &str,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    // Title (localized). An optional context string (the WiFi network name, or
    // the location breadcrumb) is shown right-aligned next to it in Latin.
    text::draw_line(
        display,
        language::text(lang, title),
        Point::new(FIELD_X, 22),
        HAlign::Left,
        crate::col_text(),
        lang,
        text::Size::Small,
    )?;
    if let Some(ctx) = context {
        text::draw_line(
            display,
            ctx,
            Point::new(FIELD_X + FIELD_W as i32, 22),
            HAlign::Right,
            crate::col_dim(),
            Language::default(),
            text::Size::Small,
        )?;
    }

    // Field box.
    Rectangle::new(Point::new(FIELD_X, FIELD_Y), Size::new(FIELD_W, FIELD_H))
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(crate::col_accent())
                .stroke_width(1)
                .fill_color(crate::col_card_bg())
                .build(),
        )
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;

    // Entered text (Latin/mono). If it overflows the box, show the tail so the
    // most-recently-typed characters stay visible. A trailing caret marks the
    // cursor. The keyboard only emits ASCII, so byte length == char count and
    // slicing on a byte boundary is safe.
    let visible = (FIELD_W as usize - 16) / 9;
    let shown: &str = if buf.len() > visible {
        &buf[buf.len() - visible..]
    } else {
        buf
    };
    let with_caret = format!("{shown}_");
    text::draw_line(
        display,
        &with_caret,
        Point::new(FIELD_X + 8, FIELD_Y + 22),
        HAlign::Left,
        crate::col_text(),
        Language::English,
        text::Size::Small,
    )?;
    Ok(())
}

fn draw_key<D>(display: &mut D, key: &PlacedKey) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    // Control keys (anything that isn't a plain character) get the accent
    // border so they stand apart from the letter grid.
    let is_control = !matches!(key.action, KeyAction::Char(_));
    let border = PrimitiveStyleBuilder::new()
        .stroke_color(if is_control {
            crate::col_accent()
        } else {
            crate::col_dim()
        })
        .stroke_width(1)
        .fill_color(crate::col_card_bg())
        .build();
    Rectangle::new(Point::new(key.x, key.y), Size::new(key.w, key.h))
        .into_styled(border)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;

    text::draw_line(
        display,
        &key.label,
        Point::new(key.x + key.w as i32 / 2, key.y + key.h as i32 / 2 + 5),
        HAlign::Center,
        crate::col_text(),
        key.label_lang,
        text::Size::Small,
    )?;
    Ok(())
}

/// Blocks until the panel is released, so the tap that opened a screen can't
/// immediately trigger a control on it (the same guard the settings screen uses).
pub fn wait_for_release<SPI>(touch: &mut Xpt2046<SPI>)
where
    SPI: SpiDevice,
{
    while matches!(touch.is_touched(), Ok(true)) {
        sleep(Duration::from_millis(20));
    }
}
