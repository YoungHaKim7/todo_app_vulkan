# WindowsOS 에서는 이게 젤 좋은데 왜 리눅스랑 틀리지?
- https://github.com/YoungHaKim7/todo_app_vulkan/commit/38fa5139b51d92633ee4c92e524f3c1398efb66c

# color는 여기서 찾자
- https://convertingcolors.com/cmyk-color-0.30_0.30_0.00_0.89.html?search=CMYK(0.30,%200.30,%200.00,%200.89)

```rust
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
/// Priority stripes left of each row's checkbox: red = emergency, yellow = next
/// up, gray = general (the default).
pub(crate) const COL_PRIO_HIGH: [f32; 4] = [0.98, 0.45, 0.43, 1.0];
pub(crate) const COL_PRIO_MID: [f32; 4] = [0.95, 0.77, 0.30, 1.0];
pub(crate) const COL_PRIO_LOW: [f32; 4] = [0.44, 0.46, 0.55, 1.0];
/// Text selection highlight inside the input field: the accent at low alpha.
pub(crate) const COL_SELECTION: [f32; 4] = [0.23, 0.52, 0.93, 0.35];
pub(crate) const COL_DANGER_HOVER: [f32; 4] = [0.98, 0.45, 0.43, 1.0];
/// Dimmer drawn behind a modal window.
pub(crate) const COL_OVERLAY: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
```
