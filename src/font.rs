//! TTF glyph rasterization for the UI font.
//!
//! The font is embedded at build time and rasterized once per UI size at startup with
//! per-pixel anti-aliasing (via `ab_glyph`); [`crate::atlas`] packs the results into a
//! single texture.
//!
//! Font: Hack Nerd Font <https://github.com/ryanoasis/nerd-fonts>, SIL Open Font License 1.1.

use ab_glyph::{FontRef, PxScale, PxScaleFont, ScaleFont};

/// Embedded font file.
pub(crate) const TTF: &[u8] = include_bytes!("../assets/font/HackNerdFont-Regular.ttf");

/// Rasterization size (pixels per em) for each [`Size`].
const PX: [f32; 2] = [20.0, 32.0];

/// UI text sizes; each maps to one rasterization pass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Size {
    Text = 0,
    Title = 1,
}

/// Number of characters in [`charset`].
pub(crate) const CHARSET_LEN: usize = 96;

/// Characters covered by the atlas: printable ASCII plus the middle dot used in hints.
pub(crate) fn charset() -> impl Iterator<Item = char> {
    (32u8..=126).map(|b| b as char).chain(std::iter::once('·'))
}

/// Maps a character to its slot index, or `None` if the font does not cover it.
pub(crate) fn char_index(c: char) -> Option<usize> {
    match c {
        ' '..='~' => Some(c as usize - 32),
        '·' => Some(95),
        _ => None,
    }
}

/// One rasterized glyph: an 8-bit coverage bitmap plus placement metrics, in pixels.
pub(crate) struct Raster {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Row-major coverage (`width * height` bytes), row 0 at the top.
    pub(crate) pixels: Vec<u8>,
    /// Left bearing: bitmap left edge relative to the pen position.
    pub(crate) left: f32,
    /// Bitmap top edge relative to the baseline (positive is up).
    pub(crate) top: f32,
}

/// Metrics for one character slot; `raster` is `None` for blank glyphs such as the space.
pub(crate) struct GlyphSlot {
    pub(crate) advance: f32,
    pub(crate) raster: Option<Raster>,
}

/// Everything rasterized for one [`Size`].
pub(crate) struct SizeRaster {
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    pub(crate) line_gap: f32,
    /// One slot per character in [`charset`].
    pub(crate) slots: Vec<GlyphSlot>,
}

/// Rasterizes every [`charset`] character at both UI sizes.
pub(crate) fn rasterize_sizes() -> [SizeRaster; 2] {
    let font = FontRef::try_from_slice(TTF).expect("embedded font is valid");
    PX.map(|px| {
        let scaled = PxScaleFont {
            font: &font,
            scale: PxScale::from(px),
        };
        rasterize(scaled)
    })
}

fn rasterize(scaled: PxScaleFont<&FontRef>) -> SizeRaster {
    let slots = charset()
        .map(|c| {
            let glyph = scaled.scaled_glyph(c);
            let advance = scaled.h_advance(glyph.id);
            let Some(outlined) = scaled.outline_glyph(glyph) else {
                return GlyphSlot {
                    advance,
                    raster: None,
                };
            };

            let bounds = outlined.px_bounds();
            let width = bounds.width().ceil().max(0.0) as u32;
            let height = bounds.height().ceil().max(0.0) as u32;
            let mut pixels = vec![0u8; (width * height) as usize];
            outlined.draw(|gx, gy, coverage| {
                if gx < width && gy < height {
                    pixels[(gy * width + gx) as usize] =
                        (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            });
            GlyphSlot {
                advance,
                raster: Some(Raster {
                    width,
                    height,
                    pixels,
                    left: bounds.min.x,
                    // Glyph space is y-down with the baseline at the glyph position, so
                    // the ink top above the baseline is the negated min.y.
                    top: -bounds.min.y,
                }),
            }
        })
        .collect();

    SizeRaster {
        ascent: scaled.ascent(),
        descent: scaled.descent(),
        line_gap: scaled.line_gap(),
        slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_covers_printable_ascii_and_middle_dot() {
        let chars: Vec<char> = charset().collect();
        assert_eq!(chars.len(), CHARSET_LEN);
        assert_eq!(char_index(' '), Some(0));
        assert_eq!(char_index('~'), Some(94));
        assert_eq!(char_index('·'), Some(95));
        assert_eq!(char_index('\n'), None);
        assert_eq!(char_index('é'), None);
    }

    #[test]
    fn rasterizes_anti_aliased_monospaced_glyphs() {
        for r in rasterize_sizes() {
            let advance_m = r.slots[char_index('M').unwrap()].advance;
            for c in charset() {
                let slot = &r.slots[char_index(c).unwrap()];
                // Hack is monospaced: every glyph shares the 'M' advance.
                assert_eq!(slot.advance, advance_m, "advance mismatch for {c:?}");
            }

            assert!(
                r.slots[char_index(' ').unwrap()].raster.is_none(),
                "space should have no bitmap"
            );

            let m = r.slots[char_index('M').unwrap()].raster.as_ref().unwrap();
            assert!(m.width > 0 && m.height > 0);
            assert!(m.top > 0.0, "'M' ink must sit above the baseline");
            assert!(m.pixels.contains(&255), "'M' should have a solid core");
            assert!(
                m.pixels.iter().any(|&p| p > 0 && p < 255),
                "'M' should have anti-aliased edge pixels"
            );
        }
    }
}
