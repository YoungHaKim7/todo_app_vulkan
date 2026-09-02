//! App settings and persistence to `settings.txt` (one `key=value` line per setting).
//!
//! Persisted: the font-size step and the last window size. Whether the settings window
//! happens to be open is per-run UI state and defaults to closed.

use std::{fs, path::Path};

use crate::font::{DEFAULT_LEVEL, LEVELS};

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
        let font_level = data
            .lines()
            .find_map(|line| {
                let value = line.strip_prefix("font=")?;
                value.trim().parse::<usize>().ok()
            })
            .unwrap_or(DEFAULT_LEVEL)
            .min(LEVELS - 1);
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
        let mut data = format!("font={}\n", self.font_level);
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
