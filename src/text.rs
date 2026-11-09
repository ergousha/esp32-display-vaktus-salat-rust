//! Localized text drawing.
//!
//! Every UI language (Turkish, English, Dutch) is written in a Latin script, so
//! all text goes through the `embedded-graphics` `iso_8859_9` (Latin-5) mono
//! fonts, which also cover the Turkish letters İ/ı/Ş/ş/Ğ/ğ/Ö/ö/Ü/ü/Ç/ç.
//! Callers pass the localized string and the active [`Language`], so a future
//! script-specific renderer can be added here without touching them.

use embedded_graphics::{
    mono_font::{
        iso_8859_9::{FONT_10X20, FONT_7X13_BOLD, FONT_9X15},
        MonoFont, MonoTextStyle,
    },
    prelude::*,
    text::{Alignment, Text},
};

use namaz_vakti_logic::language::Language;

use crate::Rgb565;

/// Text size.
#[derive(Clone, Copy)]
pub enum Size {
    /// ~15-16px — header line and secondary labels.
    Small,
    /// ~20px — status/splash lines, the next-prayer label, settings options.
    Medium,
    /// Bold ~13px — the vakit-card name row.
    CardName,
}

/// Horizontal anchoring of the text around the supplied point.
#[derive(Clone, Copy)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

fn mono_font(size: Size) -> &'static MonoFont<'static> {
    match size {
        Size::Small => &FONT_9X15,
        Size::Medium => &FONT_10X20,
        Size::CardName => &FONT_7X13_BOLD,
    }
}

/// Draws a single line of localized text, anchored at `anchor` with its
/// baseline on `anchor.y`.
pub fn draw_line<D>(
    display: &mut D,
    text: &str,
    anchor: Point,
    align: HAlign,
    color: Rgb565,
    _lang: Language,
    size: Size,
) -> anyhow::Result<()>
where
    D: DrawTarget<Color = Rgb565>,
{
    let a = match align {
        HAlign::Left => Alignment::Left,
        HAlign::Center => Alignment::Center,
        HAlign::Right => Alignment::Right,
    };
    Text::with_alignment(text, anchor, MonoTextStyle::new(mono_font(size), color), a)
        .draw(display)
        .map_err(|_| anyhow::anyhow!("draw error"))?;
    Ok(())
}
