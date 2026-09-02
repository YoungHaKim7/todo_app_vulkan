//! TTF glyph rasterization for the UI font.
//!
//! The font is embedded at build time and rasterized once per UI size at startup with
//! per-pixel anti-aliasing (via `ab_glyph`); [`crate::atlas`] packs the results into a
//! single texture. Characters the UI font does not cover (Hangul, other scripts) are
//! rasterized on demand from a system fallback font.
//!
//! Font: Hack Nerd Font <https://github.com/ryanoasis/nerd-fonts>, SIL Open Font License 1.1.

use std::sync::OnceLock;

use ab_glyph::{FontRef, FontVec, PxScale, PxScaleFont, ScaleFont};

/// Embedded font file.
pub(crate) const TTF: &[u8] = include_bytes!("../assets/font/HackNerdFont-Regular.ttf");
// pub(crate) const TTF: &[u8] = include_bytes!("../assets/font/NotoSerifKR-VariableFont_wght.ttf");

/// Number of font-size steps the user can choose between in the settings window.
pub(crate) const LEVELS: usize = 5;
/// The default step index (must rasterize at the app's original 20/32 px sizes).
pub(crate) const DEFAULT_LEVEL: usize = 2;

/// Rasterization sizes (pixels per em) per kind, one entry per level.
const PX: [[f32; LEVELS]; 2] = [
    [16.0, 18.0, 20.0, 24.0, 28.0], // text
    [26.0, 29.0, 32.0, 38.0, 44.0], // title
];

/// A UI text size: one of two kinds (body text or title) at one of [`LEVELS`] steps.
///
/// Each `(kind, level)` pair maps to one rasterization pass in the atlas; the level is
/// chosen at runtime by the settings window, so all of them are pre-rasterized at startup.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Size {
    title: bool,
    level: usize,
}

impl Size {
    pub(crate) fn text(level: usize) -> Self {
        Self {
            title: false,
            level,
        }
    }

    pub(crate) fn title(level: usize) -> Self {
        Self { title: true, level }
    }

    /// Rasterization size in pixels per em.
    pub(crate) fn px(self) -> f32 {
        PX[self.title as usize][self.level.min(LEVELS - 1)]
    }

    /// Flat index into the per-size tables of [`crate::font::rasterize_sizes`] and
    /// [`crate::atlas::FontAtlas`].
    pub(crate) fn index(self) -> usize {
        self.title as usize * LEVELS + self.level.min(LEVELS - 1)
    }
}

/// Number of characters in [`charset`].
pub(crate) const CHARSET_LEN: usize = 99;

/// Characters covered by the atlas: printable ASCII, the middle dot used in hints, the
/// true minus sign used by the font-size buttons, and the Nerd Font icons used by
/// buttons (the gear U+F013 and the pencil U+F040; their plain-Unicode counterparts
/// are not covered by the font).
pub(crate) fn charset() -> impl Iterator<Item = char> {
    (32u8..=126)
        .map(|b| b as char)
        .chain(std::iter::once('·'))
        .chain(std::iter::once('−'))
        .chain(std::iter::once('\u{f013}'))
        .chain(std::iter::once('\u{f040}'))
}

/// Maps a character to its slot index, or `None` if the font does not cover it.
pub(crate) fn char_index(c: char) -> Option<usize> {
    match c {
        ' '..='~' => Some(c as usize - 32),
        '·' => Some(95),
        '−' => Some(96),
        '\u{f013}' => Some(97),
        '\u{f040}' => Some(98),
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

/// Rasterizes every [`charset`] character at every UI size, in [`Size::index`] order.
pub(crate) fn rasterize_sizes() -> Vec<SizeRaster> {
    let font = FontRef::try_from_slice(TTF).expect("embedded font is valid");
    let mut rasters = Vec::with_capacity(2 * LEVELS);
    for &title in &[false, true] {
        for level in 0..LEVELS {
            let scaled = PxScaleFont {
                font: &font,
                scale: PxScale::from(Size { title, level }.px()),
            };
            rasters.push(rasterize(scaled));
        }
    }
    rasters
}

fn rasterize(scaled: PxScaleFont<&FontRef>) -> SizeRaster {
    let slots = charset().map(|c| raster_slot(&scaled, c)).collect();

    SizeRaster {
        ascent: scaled.ascent(),
        descent: scaled.descent(),
        line_gap: scaled.line_gap(),
        slots,
    }
}

/// Rasterizes one glyph of any scaled font into coverage pixels plus metrics.
fn raster_slot<F: ab_glyph::Font>(scaled: &PxScaleFont<&F>, c: char) -> GlyphSlot {
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
            pixels[(gy * width + gx) as usize] = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
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
}

/// System fonts tried (in order) as a source for characters the UI font lacks,
/// mainly Hangul. The `TODO_KOREAN_FONT` environment variable overrides the list.
const FALLBACK_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/NotoSansCJKkr-VF.otf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/usr/share/fonts/truetype/nanumgothic/NanumGothic.ttf",
    "/usr/share/fonts/TTF/NanumGothic.ttf",
    "/home/gy/.local/share/fonts/Google/TrueType/Noto Serif KR/Noto_Serif_KR_ExtraLight_VF_[wght].ttf",
];

/// The loaded fallback font, or `None` when no candidate exists or parses. Candidates
/// must actually cover Hangul, so a parsed-but-Latin-only font is skipped.
pub(crate) fn fallback() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(|| {
        let paths = std::env::var_os("TODO_KOREAN_FONT")
            .into_iter()
            .map(std::path::PathBuf::from)
            .chain(FALLBACK_FONTS.iter().map(std::path::PathBuf::from));
        for path in paths {
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            let Ok(font) = FontVec::try_from_vec(data) else {
                continue;
            };
            // Probe a Hangul syllable: coverage is what matters, not the name.
            if ab_glyph::Font::glyph_id(&font, '가').0 != 0 {
                println!("Korean fallback font: {}", path.display());
                return Some(font);
            }
        }
        println!("no Korean fallback font found; Hangul will render blank");
        None
    })
    .as_ref()
}

/// Rasterizes one character the UI font does not cover, at `size`, from the fallback
/// font; `None` when there is no fallback font or it lacks the glyph.
pub(crate) fn rasterize_fallback(size: Size, c: char) -> Option<GlyphSlot> {
    let font = fallback()?;
    if ab_glyph::Font::glyph_id(font, c).0 == 0 {
        return None; // not covered: render blank instead of a tofu box
    }
    let scaled = PxScaleFont {
        font,
        scale: PxScale::from(size.px()),
    };
    Some(raster_slot(&scaled, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_covers_printable_ascii_and_extra_glyphs() {
        let chars: Vec<char> = charset().collect();
        assert_eq!(chars.len(), CHARSET_LEN);
        assert_eq!(char_index(' '), Some(0));
        assert_eq!(char_index('~'), Some(94));
        assert_eq!(char_index('·'), Some(95));
        assert_eq!(char_index('−'), Some(96));
        assert_eq!(char_index('\u{f013}'), Some(97));
        assert_eq!(char_index('\u{f040}'), Some(98));
        assert_eq!(char_index('\n'), None);
        assert_eq!(char_index('é'), None);
    }

    #[test]
    fn default_level_keeps_original_sizes() {
        assert_eq!(Size::text(DEFAULT_LEVEL).px(), 20.0);
        assert_eq!(Size::title(DEFAULT_LEVEL).px(), 32.0);
    }

    #[test]
    fn size_index_lays_out_kinds_in_contiguous_blocks() {
        assert_eq!(Size::text(0).index(), 0);
        assert_eq!(Size::text(LEVELS - 1).index(), LEVELS - 1);
        assert_eq!(Size::title(0).index(), LEVELS);
        assert_eq!(Size::title(LEVELS - 1).index(), 2 * LEVELS - 1);
    }

    #[test]
    fn rasterizes_anti_aliased_monospaced_glyphs() {
        let rasters = rasterize_sizes();
        assert_eq!(rasters.len(), 2 * LEVELS);
        for r in &rasters {
            let advance_m = r.slots[char_index('M').unwrap()].advance;
            // Hack is monospaced: the regular character set shares the 'M' advance.
            // (The Nerd Font gear icon may legitimately use its own metrics.)
            for c in (32u8..=126)
                .map(|b| b as char)
                .chain(std::iter::once('·'))
                .chain(std::iter::once('−'))
            {
                let slot = &r.slots[char_index(c).unwrap()];
                assert_eq!(slot.advance, advance_m, "advance mismatch for {c:?}");
            }

            assert!(
                r.slots[char_index(' ').unwrap()].raster.is_none(),
                "space should have no bitmap"
            );
            for icon in ['\u{f013}', '\u{f040}'] {
                assert!(
                    r.slots[char_index(icon).unwrap()]
                        .raster
                        .as_ref()
                        .is_some_and(|g| g.width > 0 && g.height > 0),
                    "{icon:?} icon should have a bitmap"
                );
            }

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
