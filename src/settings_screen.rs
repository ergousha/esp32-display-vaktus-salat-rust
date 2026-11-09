//! The touch-driven settings screen and the header gear / back-arrow icons.
//!
//! Rendering follows the dashboard's "accent-highlight card" styling and only
//! draws into the top of the screen, deliberately leaving the lower half empty
//! for future options. All geometry is shared between the drawing and the
//! hit-testing helpers so a tap always maps back to exactly what was drawn.

use embedded_graphics::{
    prelude::*,
    primitives::{Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
};

use namaz_vakti_logic::language::{self, Language, Msg};

use crate::settings::Settings;
use crate::text::{self, HAlign};
use crate::{DateMode, Rgb565};

// --- Header icon (gear on the dashboard, back-arrow on the settings screen) ---
// A square tap target anchored to the right edge, vertically centred in the
// `y = 0..30` header band. The glyph itself is smaller than the tap square.
const ICON_SIZE: i32 = 30;
const ICON_X0: i32 = 480 - ICON_SIZE; // 450
const ICON_Y0: i32 = 0;
const ICON_CX: i32 = ICON_X0 + ICON_SIZE / 2; // 465
const ICON_CY: i32 = ICON_Y0 + ICON_SIZE / 2; // 15

/// `true` if a calibrated screen coordinate falls inside the header icon's tap
/// square (used to open settings on the dashboard and to hit-test the back
/// button on the settings screen).
pub fn point_in_icon(x: i32, y: i32) -> bool {
    x >= ICON_X0 && x < ICON_X0 + ICON_SIZE && y >= ICON_Y0 && y < ICON_Y0 + ICON_SIZE
}

/// Draws the minimalist "sliders" settings icon (three horizontal tracks, each
/// with a handle) in the header's top-right tap square.
pub fn draw_gear_icon<D>(display: &mut D) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let color = crate::col_text();
    let stroke = PrimitiveStyle::with_stroke(color, 2);
    let fill = PrimitiveStyle::with_fill(color);
    let hole = PrimitiveStyle::with_fill(crate::col_bg());

    let x0 = ICON_CX - 11;
    let x1 = ICON_CX + 11;
    // Three slider rows with handles at staggered positions.
    let rows = [
        (ICON_CY - 8, ICON_CX - 4),
        (ICON_CY, ICON_CX + 5),
        (ICON_CY + 8, ICON_CX - 2),
    ];
    for (y, knob_x) in rows {
        Line::new(Point::new(x0, y), Point::new(x1, y))
            .into_styled(stroke)
            .draw(display)
            .map_err(|_| anyhow::anyhow!("draw error"))?;
        let knob = Point::new(knob_x, y);
        embedded_graphics::primitives::Circle::with_center(knob, 8)
            .into_styled(fill)
            .draw(display)
            .map_err(|_| anyhow::anyhow!("draw error"))?;
        // Hollow centre so the handle reads as a ring on the track.
        embedded_graphics::primitives::Circle::with_center(knob, 3)
            .into_styled(hole)
            .draw(display)
            .map_err(|_| anyhow::anyhow!("draw error"))?;
    }
    Ok(())
}

/// Draws a left-pointing back arrow in the same tap square as the gear.
pub fn draw_back_icon<D>(display: &mut D) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let color = crate::col_text();
    let stroke = PrimitiveStyle::with_stroke(color, 2);
    let tip = Point::new(ICON_CX - 9, ICON_CY);
    // Shaft.
    Line::new(tip, Point::new(ICON_CX + 9, ICON_CY))
        .into_styled(stroke)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    // Arrow head.
    Line::new(tip, Point::new(ICON_CX - 2, ICON_CY - 7))
        .into_styled(stroke)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    Line::new(tip, Point::new(ICON_CX - 2, ICON_CY + 7))
        .into_styled(stroke)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    Ok(())
}

// --- Option box layout ---
const BOX_W: u32 = 110;
const BOX_H: u32 = 40;
const BOX_GAP: i32 = 8;
const BOX_X0: i32 = 8;
const LANG_ROW_Y: i32 = 92;
const DATE_ROW_Y: i32 = 172;

fn box_x(col: i32) -> i32 {
    BOX_X0 + col * (BOX_W as i32 + BOX_GAP)
}

// --- System row: four action buttons (WiFi, touch recalibration, About,
// Location) ---
// These are actions, not toggles. With four of them on the 480px panel they
// share the same 110px width as the option boxes above (four columns fit
// edge-to-edge: 8 + 4*110 + 3*8 = 472), so their labels are kept short (the
// recalibrate label is abbreviated to fit — see `Msg::RecalibrateTouch`).
const SYS_ROW_Y: i32 = 258;
const SYS_BOX_W: u32 = BOX_W;
const SYS_BOX_GAP: i32 = BOX_GAP;

fn sys_box_x(col: i32) -> i32 {
    BOX_X0 + col * (SYS_BOX_W as i32 + SYS_BOX_GAP)
}

/// Something the user can tap on the settings screen.
pub enum Hit {
    /// The back arrow — return to the dashboard.
    Back,
    /// Select this UI language.
    Language(Language),
    /// Select this header date mode.
    DateMode(DateMode),
    /// Open the on-device WiFi setup flow (re-provision credentials).
    Wifi,
    /// Re-run the touch-calibration wizard.
    Recalibrate,
    /// Open the About page (hardware/firmware/MAC).
    About,
    /// Open the location selection flow.
    Location,
}

/// Maps a calibrated tap to the settings control under it, if any.
pub fn hit_test(x: i32, y: i32) -> Option<Hit> {
    if point_in_icon(x, y) {
        return Some(Hit::Back);
    }
    for (col, lang) in language::Language::ALL.iter().enumerate() {
        if in_box(x, y, box_x(col as i32), LANG_ROW_Y) {
            return Some(Hit::Language(*lang));
        }
    }
    for (col, mode) in [DateMode::Miladi, DateMode::Hijri].iter().enumerate() {
        if in_box(x, y, box_x(col as i32), DATE_ROW_Y) {
            return Some(Hit::DateMode(*mode));
        }
    }
    if in_sys_box(x, y, sys_box_x(0)) {
        return Some(Hit::Wifi);
    }
    if in_sys_box(x, y, sys_box_x(1)) {
        return Some(Hit::Recalibrate);
    }
    if in_sys_box(x, y, sys_box_x(2)) {
        return Some(Hit::About);
    }
    if in_sys_box(x, y, sys_box_x(3)) {
        return Some(Hit::Location);
    }
    None
}

fn in_box(x: i32, y: i32, x0: i32, y0: i32) -> bool {
    x >= x0 && x < x0 + BOX_W as i32 && y >= y0 && y < y0 + BOX_H as i32
}

fn in_sys_box(x: i32, y: i32, x0: i32) -> bool {
    x >= x0 && x < x0 + SYS_BOX_W as i32 && y >= SYS_ROW_Y && y < SYS_ROW_Y + BOX_H as i32
}

/// Full-screen repaint of the settings screen for the current `settings`.
pub fn draw<D>(display: &mut D, settings: &Settings) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let lang = settings.language;
    display
        .clear(crate::col_bg())
        .map_err(|_| anyhow::anyhow!("draw error"))?;

    draw_back_icon(display)?;

    // Title (localized, centered).
    text::draw_line(
        display,
        language::text(lang, Msg::SettingsTitle),
        Point::new(240, 24),
        HAlign::Center,
        crate::col_text(),
        lang,
        text::Size::Medium,
    )?;

    // Section headings, aligned to the option column.
    let (heading_x, heading_align) = (BOX_X0, HAlign::Left);

    text::draw_line(
        display,
        language::text(lang, Msg::LanguageHeading),
        Point::new(heading_x, LANG_ROW_Y - 12),
        heading_align,
        crate::col_dim(),
        lang,
        text::Size::Small,
    )?;
    // Language options: each label is the language's own name (endonym).
    for (col, opt) in Language::ALL.iter().enumerate() {
        draw_option(
            display,
            box_x(col as i32),
            LANG_ROW_Y,
            language::language_label(*opt),
            *opt,
            *opt == lang,
        )?;
    }

    text::draw_line(
        display,
        language::text(lang, Msg::DateHeading),
        Point::new(heading_x, DATE_ROW_Y - 12),
        heading_align,
        crate::col_dim(),
        lang,
        text::Size::Small,
    )?;
    // Date options: labels localized to the active language.
    for (col, mode) in [DateMode::Miladi, DateMode::Hijri].iter().enumerate() {
        let msg = match mode {
            DateMode::Miladi => Msg::DateMiladi,
            DateMode::Hijri => Msg::DateHijri,
        };
        draw_option(
            display,
            box_x(col as i32),
            DATE_ROW_Y,
            language::text(lang, msg),
            lang,
            *mode == settings.date_mode,
        )?;
    }

    // System actions: WiFi setup + touch recalibration. Rendered as accent
    // buttons (never "selected") since they trigger a flow rather than toggle a
    // stored value.
    text::draw_line(
        display,
        language::text(lang, Msg::SystemHeading),
        Point::new(heading_x, SYS_ROW_Y - 12),
        heading_align,
        crate::col_dim(),
        lang,
        text::Size::Small,
    )?;
    draw_action(
        display,
        sys_box_x(0),
        language::text(lang, Msg::WifiMenu),
        lang,
    )?;
    draw_action(
        display,
        sys_box_x(1),
        language::text(lang, Msg::RecalibrateTouch),
        lang,
    )?;
    draw_action(
        display,
        sys_box_x(2),
        language::text(lang, Msg::AboutMenu),
        lang,
    )?;
    draw_action(
        display,
        sys_box_x(3),
        language::text(lang, Msg::LocationMenu),
        lang,
    )?;

    Ok(())
}

/// Draws one wide, tappable action button (WiFi setup / recalibration). Uses
/// the accent border so it reads as an actionable control rather than a
/// selectable option.
fn draw_action<D>(display: &mut D, x: i32, label: &str, lang: Language) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let border = PrimitiveStyleBuilder::new()
        .stroke_color(crate::col_accent())
        .stroke_width(1)
        .fill_color(crate::col_card_bg())
        .build();
    Rectangle::new(Point::new(x, SYS_ROW_Y), Size::new(SYS_BOX_W, BOX_H))
        .into_styled(border)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    text::draw_line(
        display,
        label,
        Point::new(x + SYS_BOX_W as i32 / 2, SYS_ROW_Y + 25),
        HAlign::Center,
        crate::col_accent(),
        lang,
        text::Size::Small,
    )?;
    Ok(())
}

/// Draws one selectable option box. `label_lang` is the language the label is
/// written in (language options render their own endonym; date options use
/// the active language).
fn draw_option<D>(
    display: &mut D,
    x: i32,
    y: i32,
    label: &str,
    label_lang: Language,
    selected: bool,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let border = PrimitiveStyleBuilder::new()
        .stroke_color(if selected {
            crate::col_accent()
        } else {
            crate::col_dim()
        })
        .stroke_width(1)
        .fill_color(if selected {
            crate::col_accent()
        } else {
            crate::col_card_bg()
        })
        .build();
    Rectangle::new(Point::new(x, y), Size::new(BOX_W, BOX_H))
        .into_styled(border)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;

    let color = if selected {
        crate::col_accent_dark()
    } else {
        crate::col_text()
    };
    text::draw_line(
        display,
        label,
        Point::new(x + BOX_W as i32 / 2, y + 25),
        HAlign::Center,
        color,
        label_lang,
        crate::text::Size::Small,
    )?;
    Ok(())
}
