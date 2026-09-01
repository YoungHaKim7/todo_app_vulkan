//! Glyph atlas: packs the rasterized font sizes into a single texture and maps characters
//! to UV quads and metrics.
//!
//! Layout: a white square at the top-left (sampled as a solid color for filled
//! rectangles), then one band of uniform grid cells per font size.
//!
//! The atlas is rasterized and packed once; [`global`] hands out the static instance.

use std::sync::OnceLock;

use crate::font::{self, Size};

/// Grid columns per size band.
const COLS: u32 = 16;
/// Transparent padding around each glyph, so linear filtering cannot bleed neighbors.
const CELL_PAD: u32 = 1;
/// Size of the solid-white cell in pixels.
const WHITE_PX: u32 = 4;

/// Placement of one glyph's bitmap inside the atlas, in pixels.
pub(crate) struct Quad {
    u: f32,
    v: f32,
    /// Bitmap size in pixels.
    pub(crate) w: f32,
    pub(crate) h: f32,
    /// Left bearing: quad left edge relative to the pen position.
    pub(crate) left: f32,
    /// Quad top edge relative to the baseline (positive is up).
    pub(crate) top: f32,
}

/// Metrics for one character slot; `quad` is `None` for blank glyphs such as the space.
pub(crate) struct Slot {
    pub(crate) advance: f32,
    quad: Option<Quad>,
}

/// Metrics for one font size.
pub(crate) struct SizeEntry {
    pub(crate) ascent: f32,
    pub(crate) line_height: f32,
    slots: Vec<Slot>,
}

/// The packed texture plus per-size, per-character metrics.
pub(crate) struct FontAtlas {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// 8-bit alpha coverage, row-major, `width * height` bytes.
    pub(crate) pixels: Vec<u8>,
    /// One entry per rasterized [`Size`], in [`Size::index`] order.
    sizes: Vec<SizeEntry>,
}

impl FontAtlas {
    pub(crate) fn size(&self, size: Size) -> &SizeEntry {
        &self.sizes[size.index()]
    }

    pub(crate) fn quad(&self, size: Size, c: char) -> Option<&Quad> {
        font::char_index(c).and_then(|i| self.size(size).slots[i].quad.as_ref())
    }

    /// Pen advance for one character; unknown characters fall back to the space's.
    pub(crate) fn advance(&self, size: Size, c: char) -> f32 {
        self.size(size).slots[font::char_index(c).unwrap_or(0)].advance
    }

    pub(crate) fn white_uv(&self) -> [f32; 2] {
        [
            WHITE_PX as f32 * 0.5 / self.width as f32,
            WHITE_PX as f32 * 0.5 / self.height as f32,
        ]
    }

    /// UV for a fractional position (`0.0..=1.0`) inside a glyph quad.
    pub(crate) fn quad_uv(&self, quad: &Quad, fx: f32, fy: f32) -> [f32; 2] {
        [
            (quad.u + fx * quad.w) / self.width as f32,
            (quad.v + fy * quad.h) / self.height as f32,
        ]
    }
}

/// The shared atlas, rasterized and packed on first use.
pub(crate) fn global() -> &'static FontAtlas {
    static ATLAS: OnceLock<FontAtlas> = OnceLock::new();
    ATLAS.get_or_init(build)
}

fn build() -> FontAtlas {
    let rasters = font::rasterize_sizes();

    // One uniform-cell band per size, stacked below the white cell.
    struct Band {
        cell_w: u32,
        cell_h: u32,
        y: u32,
    }
    let mut bands = Vec::with_capacity(rasters.len());
    let mut y = WHITE_PX;
    for r in &rasters {
        let (mut cell_w, mut cell_h) = (0, 0);
        for g in r.slots.iter().filter_map(|s| s.raster.as_ref()) {
            cell_w = cell_w.max(g.width);
            cell_h = cell_h.max(g.height);
        }
        let band = Band {
            cell_w: cell_w + 2 * CELL_PAD,
            cell_h: cell_h + 2 * CELL_PAD,
            y,
        };
        y += font::CHARSET_LEN.div_ceil(COLS as usize) as u32 * band.cell_h;
        bands.push(band);
    }

    let width = bands
        .iter()
        .map(|b| COLS * b.cell_w)
        .max()
        .unwrap_or(WHITE_PX)
        .max(WHITE_PX);
    let height = y;

    let mut pixels = vec![0u8; (width * height) as usize];
    for gy in 0..WHITE_PX {
        for gx in 0..WHITE_PX {
            pixels[(gy * width + gx) as usize] = 255;
        }
    }

    let sizes = rasters
        .iter()
        .zip(&bands)
        .map(|(r, band)| SizeEntry {
            ascent: r.ascent,
            line_height: r.ascent - r.descent + r.line_gap,
            slots: r
                .slots
                .iter()
                .enumerate()
                .map(|(i, slot)| {
                    let Some(g) = &slot.raster else {
                        return Slot {
                            advance: slot.advance,
                            quad: None,
                        };
                    };
                    let gx = (i as u32 % COLS) * band.cell_w + CELL_PAD;
                    let gy = band.y + (i as u32 / COLS) * band.cell_h + CELL_PAD;
                    for row in 0..g.height {
                        let dst = ((gy + row) * width + gx) as usize;
                        let src = row as usize * g.width as usize;
                        pixels[dst..dst + g.width as usize]
                            .copy_from_slice(&g.pixels[src..src + g.width as usize]);
                    }
                    Slot {
                        advance: slot.advance,
                        quad: Some(Quad {
                            u: gx as f32,
                            v: gy as f32,
                            w: g.width as f32,
                            h: g.height as f32,
                            left: g.left,
                            top: g.top,
                        }),
                    }
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    FontAtlas {
        width,
        height,
        pixels,
        sizes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_is_packed_and_covered() {
        let atlas = global();
        assert!(atlas.width > 0 && atlas.height > 0);
        assert_eq!(atlas.pixels.len(), (atlas.width * atlas.height) as usize);

        // The white cell is opaque.
        assert_eq!(atlas.pixels[0], 255);

        for level in 0..font::LEVELS {
            for size in [Size::text(level), Size::title(level)] {
                let entry = atlas.size(size);
                assert!(entry.ascent > 0.0);
                assert!(entry.line_height > entry.ascent);

                // Space advances but never draws; 'M' and the extra glyphs have bitmaps.
                assert!(atlas.quad(size, ' ').is_none());
                assert!(atlas.advance(size, ' ') > 0.0);
                for c in ['M', 'g', '·', '−', '\u{f013}'] {
                    let quad = atlas
                        .quad(size, c)
                        .unwrap_or_else(|| panic!("missing quad for {c:?} at {size:?}"));
                    assert!(quad.w > 0.0 && quad.h > 0.0);
                    assert!(
                        (quad.u + quad.w) <= atlas.width as f32
                            && (quad.v + quad.h) <= atlas.height as f32,
                        "quad for {c:?} overflows the atlas"
                    );
                }

                // Unknown characters still advance (fallback: the space's advance).
                assert_eq!(atlas.advance(size, 'é'), atlas.advance(size, ' '));
            }
        }
    }
}
