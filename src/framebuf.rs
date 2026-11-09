//! A RAM-backed `Rgb565` framebuffer for a single rectangular region.
//!
//! embedded-graphics primitives are rendered into a plain pixel `Vec` (no SPI
//! traffic), then the whole region is pushed to the panel in one
//! [`DrawTarget::fill_contiguous`] call — a single address-window command plus
//! one streamed pixel payload — instead of one SPI transaction per primitive.
//!
//! This is what lets the clock box repaint every second: the seven-segment
//! [`crate::segdisplay`] logic issues a `Rectangle::draw` per lit segment, and
//! against the live display each of those was its own address-window + transfer.
//! Batching them behind this buffer collapses a frame down to one transfer, and
//! the atomic flush also removes the clear-then-redraw flicker.

use core::convert::Infallible;

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};

pub struct FrameBuf {
    buf: Vec<Rgb565>,
    width: u32,
    height: u32,
}

impl FrameBuf {
    /// Allocates a `width`x`height` buffer, initially filled with `fill`.
    pub fn new(width: u32, height: u32, fill: Rgb565) -> Self {
        Self {
            buf: vec![fill; (width * height) as usize],
            width,
            height,
        }
    }

    /// Resets every pixel to `color`. Call before redrawing a frame so stale
    /// segments from the previous value don't linger.
    pub fn clear_fill(&mut self, color: Rgb565) {
        for px in self.buf.iter_mut() {
            *px = color;
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Pushes the buffer to `display` with its top-left at `top_left`, in one
    /// batched SPI transfer (`fill_contiguous` -> a single `set_pixels`).
    pub fn flush<D>(&self, display: &mut D, top_left: Point) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        display.fill_contiguous(
            &Rectangle::new(top_left, Size::new(self.width, self.height)),
            self.buf.iter().copied(),
        )
    }
}

impl DrawTarget for FrameBuf {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            // Pixels outside the buffer are silently dropped: primitives are
            // drawn in buffer-local coordinates, but clipping here keeps a
            // stray out-of-bounds draw from panicking.
            if coord.x >= 0
                && coord.y >= 0
                && (coord.x as u32) < self.width
                && (coord.y as u32) < self.height
            {
                let idx = coord.y as u32 * self.width + coord.x as u32;
                self.buf[idx as usize] = color;
            }
        }
        Ok(())
    }
}

impl OriginDimensions for FrameBuf {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}
