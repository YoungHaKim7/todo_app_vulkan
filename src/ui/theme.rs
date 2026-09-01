//! Color palette and button styles.

pub(crate) const COL_BG: [f32; 4] = [0.055, 0.06, 0.078, 1.0];
pub(crate) const COL_ROW_ALT: [f32; 4] = [0.085, 0.09, 0.115, 1.0];
pub(crate) const COL_PANEL: [f32; 4] = [0.13, 0.135, 0.17, 1.0];
pub(crate) const COL_PANEL_HOVER: [f32; 4] = [0.18, 0.19, 0.235, 1.0];
pub(crate) const COL_FIELD: [f32; 4] = [0.09, 0.095, 0.125, 1.0];
pub(crate) const COL_BORDER: [f32; 4] = [0.24, 0.25, 0.31, 1.0];
pub(crate) const COL_ACCENT: [f32; 4] = [0.23, 0.52, 0.93, 1.0];
pub(crate) const COL_ACCENT_HOVER: [f32; 4] = [0.32, 0.62, 1.0, 1.0];
pub(crate) const COL_ACCENT_DISABLED: [f32; 4] = [0.13, 0.20, 0.32, 1.0];
pub(crate) const COL_TEXT: [f32; 4] = [0.92, 0.93, 0.96, 1.0];
pub(crate) const COL_TEXT_DIM: [f32; 4] = [0.44, 0.46, 0.55, 1.0];
pub(crate) const COL_PLACEHOLDER: [f32; 4] = [0.38, 0.40, 0.48, 1.0];
pub(crate) const COL_CHECK: [f32; 4] = [0.30, 0.78, 0.49, 1.0];
pub(crate) const COL_DANGER_HOVER: [f32; 4] = [0.98, 0.45, 0.43, 1.0];

pub(crate) struct BtnStyle {
    pub(crate) base: [f32; 4],
    pub(crate) hover: [f32; 4],
    pub(crate) disabled: [f32; 4],
    pub(crate) text: [f32; 4],
}

pub(crate) const BTN_PRIMARY: BtnStyle = BtnStyle {
    base: COL_ACCENT,
    hover: COL_ACCENT_HOVER,
    disabled: COL_ACCENT_DISABLED,
    text: COL_TEXT,
};

pub(crate) const BTN_GHOST: BtnStyle = BtnStyle {
    base: COL_PANEL,
    hover: COL_PANEL_HOVER,
    disabled: COL_PANEL,
    text: COL_TEXT_DIM,
};
