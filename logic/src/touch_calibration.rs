//! Pure affine touch-calibration math: derives and applies the 6-coefficient
//! transform that maps raw XPT2046 12-bit ADC readings to logical
//! (post-rotation) screen pixels.
//!
//! Kept hardware-free (like the rest of this crate) so the least-squares solve
//! can be unit tested on a plain host toolchain. The firmware side — NVS
//! persistence and the on-screen calibration wizard — lives in
//! `src/touch_calibration.rs` and re-exports [`Calibration`] from here.

/// Affine map from raw ADC space to logical screen space:
///
/// ```text
/// x_screen = a·x_raw + b·y_raw + c
/// y_screen = d·x_raw + e·y_raw + f
/// ```
///
/// The cross terms (`b`, `d`) let the fit absorb the slight rotation/skew a
/// resistive panel can have relative to the LCD underneath it — an axis-aligned
/// two-point calibration can't.
///
/// Persisted with [`Calibration::to_bytes`]/[`Calibration::from_bytes`] rather
/// than serde: the Xtensa LLVM backend in the ESP Rust fork miscompiles
/// `serde_json`'s float parsing, and a fixed 24-byte numeric record is a better
/// fit for six coefficients than JSON anyway.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

/// One calibration observation: the (averaged) raw ADC reading captured while
/// the user tapped a target at the known logical screen position
/// `(screen_x, screen_y)`.
#[derive(Clone, Copy, Debug)]
pub struct CalPoint {
    pub raw_x: f64,
    pub raw_y: f64,
    pub screen_x: f64,
    pub screen_y: f64,
}

impl Calibration {
    /// Maps a raw ADC reading to a logical screen pixel (rounded to integer
    /// coordinates). This is the entry point future touch-driven UI calls.
    pub fn to_screen(&self, raw_x: u16, raw_y: u16) -> (i32, i32) {
        let (x, y) = self.predict(raw_x as f64, raw_y as f64);
        (x.round() as i32, y.round() as i32)
    }

    /// Predicts the un-rounded screen position for a raw reading. Used by the
    /// wizard's center-point verification, which compares against the target
    /// with sub-pixel precision.
    pub fn predict(&self, raw_x: f64, raw_y: f64) -> (f64, f64) {
        let x = self.a as f64 * raw_x + self.b as f64 * raw_y + self.c as f64;
        let y = self.d as f64 * raw_x + self.e as f64 * raw_y + self.f as f64;
        (x, y)
    }

    /// Serializes the 6 coefficients to a fixed 24-byte little-endian blob for
    /// NVS storage (see the note on [`Calibration`] for why this isn't serde).
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut out = [0u8; 24];
        let fields = [self.a, self.b, self.c, self.d, self.e, self.f];
        for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
            *chunk = v.to_le_bytes();
        }
        out
    }

    /// Inverse of [`Self::to_bytes`]; returns `None` unless `bytes` is exactly
    /// 24 bytes (a truncated/foreign blob).
    pub fn from_bytes(bytes: &[u8]) -> Option<Calibration> {
        if bytes.len() != 24 {
            return None;
        }
        let rd =
            |i: usize| f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        Some(Calibration {
            a: rd(0),
            b: rd(4),
            c: rd(8),
            d: rd(12),
            e: rd(16),
            f: rd(20),
        })
    }
}

/// Solves the (over-determined) affine system from `points` by least squares.
///
/// With the standard 4-corner capture this is 8 equations for 6 unknowns, which
/// is more robust to per-point noise than an exact 3-point solve. Needs at
/// least 3 non-collinear points; returns `None` if the points are degenerate
/// (singular normal matrix).
pub fn solve_affine(points: &[CalPoint]) -> Option<Calibration> {
    if points.len() < 3 {
        return None;
    }

    // Each design-matrix row is r = [raw_x, raw_y, 1]. Accumulate the 3×3
    // normal matrix m = Σ rᵀ·r (symmetric, shared by both coordinate fits) and
    // the two right-hand sides bx = Σ r·screen_x, by = Σ r·screen_y.
    let mut m = [[0.0f64; 3]; 3];
    let mut bx = [0.0f64; 3];
    let mut by = [0.0f64; 3];
    for p in points {
        let r = [p.raw_x, p.raw_y, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += r[i] * r[j];
            }
            bx[i] += r[i] * p.screen_x;
            by[i] += r[i] * p.screen_y;
        }
    }

    let cx = solve3(&m, &bx)?;
    let cy = solve3(&m, &by)?;
    Some(Calibration {
        a: cx[0] as f32,
        b: cx[1] as f32,
        c: cx[2] as f32,
        d: cy[0] as f32,
        e: cy[1] as f32,
        f: cy[2] as f32,
    })
}

/// Solves a 3×3 linear system `m·x = b` by Gaussian elimination with partial
/// pivoting. Returns `None` if the matrix is singular (degenerate points).
// Index-based loops read clearest here: elimination touches two distinct rows
// per step (`a[r][c]` written from `a[col][c]`), which iterator adapters obscure.
#[allow(clippy::needless_range_loop)]
fn solve3(m: &[[f64; 3]; 3], b: &[f64; 3]) -> Option<[f64; 3]> {
    // Augmented matrix [m | b].
    let mut a = [
        [m[0][0], m[0][1], m[0][2], b[0]],
        [m[1][0], m[1][1], m[1][2], b[1]],
        [m[2][0], m[2][1], m[2][2], b[2]],
    ];

    for col in 0..3 {
        // Partial pivot: move the largest-magnitude entry in this column
        // (at or below the diagonal) onto the diagonal for numerical stability.
        let mut piv = col;
        for r in (col + 1)..3 {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        if a[piv][col].abs() < 1e-9 {
            return None; // singular — degenerate/collinear points
        }
        a.swap(col, piv);

        for r in (col + 1)..3 {
            let factor = a[r][col] / a[col][col];
            for c in col..4 {
                a[r][c] -= factor * a[col][c];
            }
        }
    }

    // Back-substitution.
    let mut x = [0.0f64; 3];
    for i in (0..3).rev() {
        let mut sum = a[i][3];
        for j in (i + 1)..3 {
            sum -= a[i][j] * x[j];
        }
        x[i] = sum / a[i][i];
    }
    Some(x)
}

/// Documented vendor-default raw ADC extents (reverse-engineered from the
/// sibling C/LVGL project — see the README pinout table), both axes inverted.
pub const VENDOR_RAW_X_MIN: f64 = 110.0;
pub const VENDOR_RAW_X_MAX: f64 = 1971.0;
pub const VENDOR_RAW_Y_MIN: f64 = 88.0;
pub const VENDOR_RAW_Y_MAX: f64 = 1929.0;

/// Builds an axis-aligned fallback calibration from the vendor-default ADC
/// extents for a `width`×`height` logical screen, both axes inverted to match
/// the documented panel orientation.
///
/// This is a best-effort guess for the no-stylus / repeated-failure edge cases
/// only — it is **not** a substitute for running the wizard (resistive panels
/// vary unit to unit) and, being reverse-engineered without hardware, may need
/// its axes swapped or flipped on a real board.
pub fn vendor_default(width: u16, height: u16) -> Calibration {
    // x_screen = a·raw_x + c, inverted: raw_x MAX → 0, raw_x MIN → width-1.
    let a = -((width as f64 - 1.0) / (VENDOR_RAW_X_MAX - VENDOR_RAW_X_MIN));
    let c = -a * VENDOR_RAW_X_MAX;
    // y_screen = e·raw_y + f, inverted: raw_y MAX → 0, raw_y MIN → height-1.
    let e = -((height as f64 - 1.0) / (VENDOR_RAW_Y_MAX - VENDOR_RAW_Y_MIN));
    let f = -e * VENDOR_RAW_Y_MAX;
    Calibration {
        a: a as f32,
        b: 0.0,
        c: c as f32,
        d: 0.0,
        e: e as f32,
        f: f as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Screen dimensions of the logical (post-rotation) landscape canvas.
    const W: u16 = 480;
    const H: u16 = 320;

    fn points_from(truth: &Calibration, raws: &[(f64, f64)]) -> Vec<CalPoint> {
        raws.iter()
            .map(|&(rx, ry)| {
                let (sx, sy) = truth.predict(rx, ry);
                CalPoint {
                    raw_x: rx,
                    raw_y: ry,
                    screen_x: sx,
                    screen_y: sy,
                }
            })
            .collect()
    }

    fn assert_close(cal: &Calibration, truth: &Calibration, tol: f32) {
        for (got, want) in [
            (cal.a, truth.a),
            (cal.b, truth.b),
            (cal.c, truth.c),
            (cal.d, truth.d),
            (cal.e, truth.e),
            (cal.f, truth.f),
        ] {
            assert!((got - want).abs() < tol, "coeff {got} vs {want}");
        }
    }

    #[test]
    fn recovers_exact_affine_from_four_corners() {
        // A plausible transform with real cross-terms (slight panel skew).
        let truth = Calibration {
            a: 0.256,
            b: 0.004,
            c: -28.5,
            d: -0.006,
            e: -0.171,
            f: 330.0,
        };
        // Raw readings the panel would produce for each corner target.
        let raws = [
            (1900.0, 1850.0),
            (450.0, 1840.0),
            (1905.0, 200.0),
            (455.0, 210.0),
        ];
        let cal = solve_affine(&points_from(&truth, &raws)).expect("solvable");
        assert_close(&cal, &truth, 1e-2);
    }

    #[test]
    fn round_trips_targets_within_a_pixel() {
        let truth = Calibration {
            a: 0.255,
            b: 0.0,
            c: -30.0,
            d: 0.0,
            e: -0.170,
            f: 328.0,
        };
        let raws = [
            (1900.0, 1850.0),
            (450.0, 1850.0),
            (1900.0, 200.0),
            (450.0, 200.0),
        ];
        let pts = points_from(&truth, &raws);
        let cal = solve_affine(&pts).unwrap();
        for p in &pts {
            let (sx, sy) = cal.to_screen(p.raw_x as u16, p.raw_y as u16);
            assert!((sx - p.screen_x.round() as i32).abs() <= 1);
            assert!((sy - p.screen_y.round() as i32).abs() <= 1);
        }
    }

    #[test]
    fn least_squares_averages_out_noise() {
        // Over-determined fit should recover the underlying transform closely
        // even when each captured screen point is perturbed by a pixel or two.
        let truth = Calibration {
            a: 0.256,
            b: 0.0,
            c: -30.0,
            d: 0.0,
            e: -0.171,
            f: 330.0,
        };
        let raws = [
            (1900.0, 1850.0),
            (450.0, 1850.0),
            (1900.0, 200.0),
            (450.0, 200.0),
        ];
        let noise = [(1.5, -1.0), (-1.0, 1.5), (1.0, 1.0), (-1.5, -1.5)];
        let mut pts = points_from(&truth, &raws);
        for (p, n) in pts.iter_mut().zip(noise.iter()) {
            p.screen_x += n.0;
            p.screen_y += n.1;
        }
        let cal = solve_affine(&pts).unwrap();
        // The independent center tap should still predict near the true center.
        let center_raw = (1175.0, 1025.0);
        let (px, py) = cal.predict(center_raw.0, center_raw.1);
        let (tx, ty) = truth.predict(center_raw.0, center_raw.1);
        assert!((px - tx).abs() < 3.0, "center x {px} vs {tx}");
        assert!((py - ty).abs() < 3.0, "center y {py} vs {ty}");
    }

    #[test]
    fn rejects_degenerate_points() {
        // Too few points.
        assert!(solve_affine(&[]).is_none());
        // All samples identical (rank-1 normal matrix).
        let same = CalPoint {
            raw_x: 1000.0,
            raw_y: 1000.0,
            screen_x: 240.0,
            screen_y: 160.0,
        };
        assert!(solve_affine(&[same, same, same, same]).is_none());
        // Collinear raw samples can't fix both axes.
        let collinear: Vec<CalPoint> = (0..4)
            .map(|i| {
                let t = i as f64 * 300.0;
                CalPoint {
                    raw_x: 200.0 + t,
                    raw_y: 200.0 + t,
                    screen_x: 10.0 + t,
                    screen_y: 10.0 + t,
                }
            })
            .collect();
        assert!(solve_affine(&collinear).is_none());
    }

    #[test]
    fn bytes_round_trip_exactly() {
        let cal = Calibration {
            a: 0.256,
            b: -0.004,
            c: -28.5,
            d: 0.006,
            e: -0.171,
            f: 330.25,
        };
        let bytes = cal.to_bytes();
        assert_eq!(bytes.len(), 24);
        // f32 bit patterns round-trip losslessly.
        assert_eq!(Calibration::from_bytes(&bytes), Some(cal));
        // Wrong lengths are rejected rather than misread.
        assert_eq!(Calibration::from_bytes(&bytes[..23]), None);
        assert_eq!(Calibration::from_bytes(&[]), None);
    }

    #[test]
    fn vendor_default_maps_extents_to_inverted_edges() {
        let cal = vendor_default(W, H);
        // X inverted: raw MAX → 0, raw MIN → width-1.
        let (x_at_max, _) = cal.to_screen(VENDOR_RAW_X_MAX as u16, VENDOR_RAW_Y_MAX as u16);
        let (x_at_min, _) = cal.to_screen(VENDOR_RAW_X_MIN as u16, VENDOR_RAW_Y_MIN as u16);
        assert_eq!(x_at_max, 0);
        assert_eq!(x_at_min, W as i32 - 1);
        // Y inverted: raw MAX → 0, raw MIN → height-1.
        let (_, y_at_max) = cal.to_screen(VENDOR_RAW_X_MAX as u16, VENDOR_RAW_Y_MAX as u16);
        let (_, y_at_min) = cal.to_screen(VENDOR_RAW_X_MIN as u16, VENDOR_RAW_Y_MIN as u16);
        assert_eq!(y_at_max, 0);
        assert_eq!(y_at_min, H as i32 - 1);
    }
}
