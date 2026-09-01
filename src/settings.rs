//! App settings and persistence to `settings.txt` (one `key=value` line per setting).
//!
//! Only the font-size step is persisted; whether the settings window happens to be open
//! is per-run UI state and defaults to closed.

use std::{fs, path::Path};

use crate::font::{DEFAULT_LEVEL, LEVELS};

pub(crate) struct Settings {
    /// Whether the settings window is currently open.
    pub(crate) open: bool,
    /// Font-size step index; each step rasterizes the whole UI one size up or down.
    pub(crate) font_level: usize,
}

impl Settings {
    pub(crate) fn load(path: &Path) -> Self {
        let font_level = fs::read_to_string(path)
            .ok()
            .and_then(|data| {
                data.lines().find_map(|line| {
                    let value = line.strip_prefix("font=")?;
                    value.trim().parse::<usize>().ok()
                })
            })
            .unwrap_or(DEFAULT_LEVEL)
            .min(LEVELS - 1);
        Self {
            open: false,
            font_level,
        }
    }

    pub(crate) fn save(&self, path: &Path) {
        let _ = fs::write(path, format!("font={}\n", self.font_level));
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

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vulkan_todo_settings_{}.txt", std::process::id()))
    }

    #[test]
    fn missing_file_falls_back_to_default() {
        let path = std::env::temp_dir().join("vulkan_todo_settings_absent.txt");
        let _ = fs::remove_file(&path);
        let settings = Settings::load(&path);
        assert!(!settings.open);
        assert_eq!(settings.font_level, DEFAULT_LEVEL);
    }

    #[test]
    fn save_file_roundtrip_and_clamping() {
        let path = temp_path();
        let mut settings = Settings {
            open: true,
            font_level: 0,
        };
        settings.set_font_level(99, &path);
        assert_eq!(settings.font_level, LEVELS - 1);

        settings.set_font_level(1, &path);
        let loaded = Settings::load(&path);
        assert_eq!(loaded.font_level, 1);
        assert!(!loaded.open, "open state must not persist");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn garbage_file_falls_back_to_default() {
        let path = temp_path();
        fs::write(&path, "hello world\nfont=abc\n").unwrap();
        assert_eq!(Settings::load(&path).font_level, DEFAULT_LEVEL);
        let _ = fs::remove_file(&path);
    }
}
