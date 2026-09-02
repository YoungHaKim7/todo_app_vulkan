//! App settings and persistence to `settings.txt` (one `key=value` line per setting).
//!
//! Persisted: the font-size step and the last window size. Whether the settings window
//! happens to be open is per-run UI state and defaults to closed.

use std::{fs, path::Path};

use crate::font::{DEFAULT_LEVEL, LEVELS};

/// Settings-file format version, written as a `v=` line. Files without the line are
/// the original format; each version bumped the font-level scheme, so older files
/// are migrated to the current one on load.
const FORMAT_VERSION: usize = 3;

/// Maps the five font steps of the version-1 format to the current twenty-step
/// scheme by pixel size (v1 steps were 16/18/20/24/28 px of text).
const V1_LEVEL_MAP: [usize; 5] = [7, 8, 9, 11, 13];
/// Offset from a version-2 font level (eleven steps, 10..30 px of text) to the
/// current scheme, which keeps those sizes as levels 4..14.
const V2_LEVEL_OFFSET: usize = 4;

pub(crate) struct Settings {
    /// Whether the settings window is currently open.
    pub(crate) open: bool,
    /// Font-size step index; each step rasterizes the whole UI one size up or down.
    pub(crate) font_level: usize,
    /// Last window size in logical units, so the next run opens at the size the user
    /// left the window. `None` while the window has not been resized, so a first run
    /// (or a never-resized one) does not write a window line to disk.
    pub(crate) window_size: Option<[u32; 2]>,
}

impl Settings {
    pub(crate) fn load(path: &Path) -> Self {
        let data = fs::read_to_string(path).unwrap_or_default();
        let version = data
            .lines()
            .find_map(|line| line.strip_prefix("v=")?.trim().parse::<usize>().ok());
        let font_level = data
            .lines()
            .find_map(|line| {
                let value = line.strip_prefix("font=")?;
                value.trim().parse::<usize>().ok()
            })
            .map(|level| match version {
                // Older files are re-indexed into the current scheme by pixel size:
                // v1 stored one of five steps, v2 one of eleven.
                None => V1_LEVEL_MAP[level.min(V1_LEVEL_MAP.len() - 1)],
                Some(2) => level.min(10) + V2_LEVEL_OFFSET,
                Some(_) => level.min(LEVELS - 1),
            })
            .unwrap_or(DEFAULT_LEVEL);
        let window_size = data.lines().find_map(|line| {
            let value = line.strip_prefix("window=")?;
            let (w, h) = value.trim().split_once('x')?;
            let size = [w.parse::<u32>().ok()?, h.parse::<u32>().ok()?];
            (size[0] > 0 && size[1] > 0).then_some(size)
        });
        Self {
            open: false,
            font_level,
            window_size,
        }
    }

    pub(crate) fn save(&self, path: &Path) {
        let mut data = format!("v={FORMAT_VERSION}\nfont={}\n", self.font_level);
        if let Some([w, h]) = self.window_size {
            data.push_str(&format!("window={w}x{h}\n"));
        }
        let _ = fs::write(path, data);
    }

    /// Steps the font size and persists it; out-of-range steps are clamped, not ignored,
    /// so the saved value always matches what the UI shows.
    pub(crate) fn set_font_level(&mut self, level: usize, path: &Path) {
        self.font_level = level.min(LEVELS - 1);
        self.save(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test temp path: tests run in parallel and share a process id, so the name
    /// must differ per test or one test's cleanup deletes another's fixture.
    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "vulkan_todo_settings_{tag}_{}.txt",
            std::process::id()
        ))
    }

    #[test]
    fn missing_file_falls_back_to_default() {
        let path = temp_path("absent");
        let _ = fs::remove_file(&path);
        let settings = Settings::load(&path);
        assert!(!settings.open);
        assert_eq!(settings.font_level, DEFAULT_LEVEL);
        assert_eq!(settings.window_size, None);
    }

    #[test]
    fn save_file_roundtrip_and_clamping() {
        let path = temp_path("roundtrip");
        let mut settings = Settings {
            open: true,
            font_level: 0,
            window_size: Some([1280, 800]),
        };
        settings.set_font_level(99, &path);
        assert_eq!(settings.font_level, LEVELS - 1);

        settings.set_font_level(1, &path);
        let loaded = Settings::load(&path);
        assert_eq!(loaded.font_level, 1);
        assert_eq!(loaded.window_size, Some([1280, 800]));
        assert!(!loaded.open, "open state must not persist");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn old_format_font_level_is_remapped() {
        let path = temp_path("migrate");
        // No `v=` line: a v1 file. Level 4 was 28 px of text, which is level 13 now.
        fs::write(&path, "font=4\nwindow=640x480\n").unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.font_level, 13);
        assert_eq!(settings.window_size, Some([640, 480]));
        // An out-of-range v1 level clamps into the mapping table, as v1 saves did.
        fs::write(&path, "font=99\n").unwrap();
        assert_eq!(Settings::load(&path).font_level, V1_LEVEL_MAP[4]);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn v2_format_font_level_is_remapped() {
        let path = temp_path("migrate_v2");
        // A v2 file's level 4 was 18 px of text; in the twenty-step scheme that is 8.
        fs::write(&path, "v=2\nfont=4\n").unwrap();
        assert_eq!(Settings::load(&path).font_level, 8);
        // Out-of-range v2 levels clamp to the v2 maximum before the offset, as v2
        // saves did.
        fs::write(&path, "v=2\nfont=99\n").unwrap();
        assert_eq!(Settings::load(&path).font_level, 10 + V2_LEVEL_OFFSET);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn garbage_file_falls_back_to_default() {
        let path = temp_path("garbage");
        fs::write(&path, "hello world\nfont=abc\n").unwrap();
        assert_eq!(Settings::load(&path).font_level, DEFAULT_LEVEL);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn garbage_or_zero_window_line_is_ignored() {
        for (tag, contents) in [
            ("junk", "window=nonsense\n"),
            ("partial", "window=1024x\n"),
            ("zero", "window=0x768\n"),
        ] {
            let path = temp_path(tag);
            fs::write(&path, contents).unwrap();
            assert_eq!(
                Settings::load(&path).window_size,
                None,
                "{contents} must not parse as a window size"
            );
            let _ = fs::remove_file(&path);
        }
    }
}
