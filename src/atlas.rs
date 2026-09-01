//! Glyph atlas: packs the rasterized font sizes into a single texture and maps characters
//! to UV quads and metrics.
//!
//! Layout: a white square at the top-left (sampled as a solid color for filled
//! rectangles), then one band of uniform grid cells per font size, and below those a
//! growing shelf-packed region for glyphs rasterized on demand (Hangul and anything
//! else the UI font does not cover).
//!
//! The static part is rasterized and packed once; [`global`] hands out that instance.
//! The dynamic part grows downward in the same pixel buffer and bumps a generation
//! counter whenever its content changes, so the renderer knows to re-upload.

use std::{collections::HashMap, sync::{Mutex, OnceLock}};

use crate::font::{self, GlyphSlot, Size};

/// Grid columns per size band.
const COLS: u32 = 16;
/// Transparent padding around each glyph, so linear filtering cannot bleed neighbors.
const CELL_PAD: u32 = 1;
/// Size of the solid-white cell in pixels.
const WHITE_PX: u32 = 4;
/// Fixed texture width: wide enough for the static bands plus room for dynamic rows.
const TEXTURE_W: u32 = 1024;
/// Dynamic region height added on first use and each time it fills, until this cap.
const DYN_GROW: u32 = 1024;
/// Hard cap on the whole texture height (16 MiB of R8 pixels at 1024 wide).
const TEXTURE_H_MAX: u32 = 16384;

/// Placement of one glyph's bitmap inside the atlas, in pixels.
#[derive(Clone)]
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

    fn static_quad(&self, size: Size, c: char) -> Option<&Quad> {
        font::char_index(c).and_then(|i| self.size(size).slots[i].quad.as_ref())
    }

    /// Pen advance for one character; unknown characters fall back to the space's.
    fn static_advance(&self, size: Size, c: char) -> f32 {
        self.size(size).slots[font::char_index(c).unwrap_or(0)].advance
    }
}

/// The shared static atlas, rasterized and packed on first use.
pub(crate) fn global() -> &'static FontAtlas {
    static ATLAS: OnceLock<FontAtlas> = OnceLock::new();
    ATLAS.get_or_init(build)
}

/// The on-demand glyph region: shelf-packed rows below the static bands, in the same
/// pixel buffer. Content changes bump `generation`.
struct Dynamic {
    /// Total texture height in pixels; the buffer holds `TEXTURE_W * height` bytes.
    height: u32,
    pixels: Vec<u8>,
    /// Top of the shelf currently being filled, and its row height so far.
    shelf_y: u32,
    shelf_h: u32,
    /// Next free x inside the current shelf.
    cursor_x: u32,
    entries: HashMap<(usize, char), Slot>,
    /// Set once the texture cap is reached; later glyphs render blank, not tofu.
    full: bool,
    generation: u64,
}

impl Dynamic {
    /// Creates the region below the static bands, copying their pixels (at identical
    /// x/y, only the row stride widens) into the full-width buffer.
    fn new(base: &FontAtlas) -> Self {
        let height = (base.height + DYN_GROW).min(TEXTURE_H_MAX);
        let mut pixels = vec![0u8; (TEXTURE_W * height) as usize];
        for row in 0..base.height {
            let dst = (row * TEXTURE_W) as usize;
            let src = (row * base.width) as usize;
            pixels[dst..dst + base.width as usize]
                .copy_from_slice(&base.pixels[src..src + base.width as usize]);
        }
        Self {
            height,
            pixels,
            shelf_y: base.height,
            shelf_h: 0,
            cursor_x: 0,
            entries: HashMap::new(),
            full: false,
            generation: 1,
        }
    }

    fn ensure(&mut self, base: &FontAtlas, size: Size, c: char) -> &Slot {
        let key = (size.index(), c);
        if !self.entries.contains_key(&key) {
            let slot = self.rasterize_into(base, size, c);
            self.entries.insert(key, slot);
        }
        &self.entries[&key]
    }

    /// Rasterizes `c` from the fallback font and blits it into the next free cell.
    fn rasterize_into(&mut self, base: &FontAtlas, size: Size, c: char) -> Slot {
        let slot = font::rasterize_fallback(size, c).unwrap_or(GlyphSlot {
            // No fallback font or glyph: keep the pen moving like the static atlas
            // does for unknown characters.
            advance: base.static_advance(size, ' '),
            raster: None,
        });
        let Some(g) = slot.raster else {
            return Slot {
                advance: slot.advance,
                quad: None,
            };
        };

        let (cell_w, cell_h) = (g.width + 2 * CELL_PAD, g.height + 2 * CELL_PAD);
        let (gx, gy) = match self.reserve(cell_w, cell_h) {
            Some(pos) => pos,
            None => {
                // Texture cap reached; stop trying to place more bitmaps.
                return Slot {
                    advance: slot.advance,
                    quad: None,
                };
            }
        };
        for row in 0..g.height {
            let dst = ((gy + row) * TEXTURE_W + gx) as usize;
            let src = row as usize * g.width as usize;
            self.pixels[dst..dst + g.width as usize]
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
    }

    /// Shelf-packs a cell of the given size, growing the buffer when needed; `None`
    /// when the texture cap leaves no room.
    fn reserve(&mut self, cell_w: u32, cell_h: u32) -> Option<(u32, u32)> {
        if self.full {
            return None;
        }
        if self.cursor_x + cell_w > TEXTURE_W {
            self.shelf_y += self.shelf_h;
            self.cursor_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + cell_h > self.height {
            if self.height >= TEXTURE_H_MAX {
                self.full = true;
                return None;
            }
            self.height = (self.height + DYN_GROW).min(TEXTURE_H_MAX);
            self.pixels
                .resize((TEXTURE_W * self.height) as usize, 0);
        }
        let pos = (self.cursor_x + CELL_PAD, self.shelf_y + CELL_PAD);
        self.cursor_x += cell_w;
        self.shelf_h = self.shelf_h.max(cell_h);
        self.generation += 1;
        Some(pos)
    }
}

static DYNAMIC: Mutex<Option<Dynamic>> = Mutex::new(None);

/// Runs `f` against the dynamic region, creating it below the static bands first.
fn with_dynamic<R>(f: impl FnOnce(&mut Dynamic) -> R) -> R {
    let mut guard = DYNAMIC.lock().unwrap_or_else(|e| e.into_inner());
    let dyn_atlas = guard.get_or_insert_with(|| Dynamic::new(global()));
    f(dyn_atlas)
}

/// Center of the solid-white cell, for sampling a flat fill color.
pub(crate) fn white_uv() -> [f32; 2] {
    let (_, h) = dimensions();
    [
        WHITE_PX as f32 * 0.5 / TEXTURE_W as f32,
        WHITE_PX as f32 * 0.5 / h as f32,
    ]
}

/// Placement and metrics for one character at one size, from whichever layer covers
/// it. Characters outside both fonts return the space advance and no quad.
pub(crate) fn quad(size: Size, c: char) -> Option<Quad> {
    if let Some(q) = global().static_quad(size, c) {
        return Some(q.clone());
    }
    with_dynamic(|d| d.ensure(global(), size, c).quad.clone())
}

pub(crate) fn advance(size: Size, c: char) -> f32 {
    let base = global();
    match font::char_index(c) {
        Some(_) => base.static_advance(size, c),
        None => with_dynamic(|d| d.ensure(base, size, c).advance),
    }
}

/// Bumped whenever the atlas content changed since the last upload.
pub(crate) fn generation() -> u64 {
    with_dynamic(|d| d.generation)
}

/// Current texture size in pixels.
pub(crate) fn dimensions() -> (u32, u32) {
    with_dynamic(|d| (TEXTURE_W, d.height))
}

/// Snapshot of the whole texture for GPU upload.
pub(crate) fn pixels() -> Vec<u8> {
    with_dynamic(|d| d.pixels.clone())
}

/// UV for a fractional position (`0.0..=1.0`) inside a glyph quad.
pub(crate) fn quad_uv(quad: &Quad, fx: f32, fy: f32) -> [f32; 2] {
    let (w, h) = dimensions();
    [
        (quad.u + fx * quad.w) / w as f32,
        (quad.v + fy * quad.h) / h as f32,
    ]
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
        .max(WHITE_PX)
        .max(TEXTURE_W);
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

        let (tw, th) = dimensions();
        assert_eq!(tw, TEXTURE_W);
        assert!(th >= atlas.height);

        for level in 0..font::LEVELS {
            for size in [Size::text(level), Size::title(level)] {
                let entry = atlas.size(size);
                assert!(entry.ascent > 0.0);
                assert!(entry.line_height > entry.ascent);

                // Space advances but never draws; 'M' and the extra glyphs have bitmaps.
                assert!(quad(size, ' ').is_none());
                assert!(advance(size, ' ') > 0.0);
                for c in ['M', 'g', '·', '−', '\u{f013}'] {
                    let q = quad(size, c)
                        .unwrap_or_else(|| panic!("missing quad for {c:?} at {size:?}"));
                    assert!(q.w > 0.0 && q.h > 0.0);
                    assert!(
                        (q.u + q.w) <= tw as f32 && (q.v + q.h) <= th as f32,
                        "quad for {c:?} overflows the atlas"
                    );
                }

                // Characters beyond the UI font now come from the fallback font with
                // real metrics; only fully uncovered ones fall back to the space.
                assert!(advance(size, 'é') > 0.0);
            }
        }
    }

    #[test]
    fn dynamic_glyphs_rasterize_on_demand() {
        let size = Size::text(font::DEFAULT_LEVEL);

        // Regardless of fallback font presence, unassigned codepoints have no glyph.
        let uncovered = '\u{0378}';
        assert!(quad(size, uncovered).is_none());
        assert_eq!(advance(size, uncovered), advance(size, ' '));

        if font::fallback().is_none() {
            return; // no Korean font on this machine
        }

        let before = generation();
        let q = quad(size, '한').expect("Hangul syllable needs a quad");
        assert!(generation() > before, "new glyph must bump the generation");
        assert!(q.w > 0.0 && q.h > 0.0, "Hangul glyph has ink");

        // Placed inside the texture, below the static bands, without overlap.
        let (tw, th) = dimensions();
        assert!(q.v >= global().height as f32, "below the static bands");
        assert!(q.u + q.w <= tw as f32 && q.v + q.h <= th as f32);

        // The blitted pixels are actually there and non-empty.
        let px = pixels();
        let mut ink = 0u32;
        for y in 0..q.h as u32 {
            for x in 0..q.w as u32 {
                if px[((q.v as u32 + y) * tw + q.u as u32 + x) as usize] > 128 {
                    ink += 1;
                }
            }
        }
        assert!(ink > 0, "'한' should have solid pixels");

        // Second lookup is a cache hit: no further generation bump. (Other tests in
        // this binary do not touch '한', so only this lookup can change the count.)
        let mid = generation();
        let _ = quad(size, '한');
        assert_eq!(generation(), mid);

        // A distinct size gets its own entry.
        let other = Size::title(font::DEFAULT_LEVEL);
        let before = generation();
        assert!(quad(other, '한').is_some());
        assert!(generation() > before);

        // Advance comes from the fallback font, not the space fallback.
        assert!(advance(size, '한') > 0.0);
    }
}
