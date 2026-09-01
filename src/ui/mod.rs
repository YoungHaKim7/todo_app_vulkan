//! Immediate-mode UI core: vertex building, rectangles, and TTF-glyph text.
//!
//! Widgets and screen layout live in the [`widgets`] and [`screen`] submodules; colors in
//! [`theme`].

pub(crate) mod screen;
pub(crate) mod theme;
pub(crate) mod widgets;

use vulkano::{buffer::BufferContents, pipeline::graphics::vertex_input::Vertex};

use crate::{
    atlas,
    font::{DEFAULT_LEVEL, Size},
};

#[derive(BufferContents, Clone, Copy, Vertex)]
#[repr(C)]
pub(crate) struct UiVertex {
    #[format(R32G32_SFLOAT)]
    pos: [f32; 2],
    #[format(R32G32_SFLOAT)]
    uv: [f32; 2],
    #[format(R32G32B32A32_SFLOAT)]
    color: [f32; 4],
}

#[derive(Clone, Copy)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Rect {
    pub(crate) fn contains(self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] < self.x + self.w && p[1] >= self.y && p[1] < self.y + self.h
    }

    pub(crate) fn inset(self, d: f32) -> Rect {
        Rect {
            x: self.x + d,
            y: self.y + d,
            w: self.w - 2.0 * d,
            h: self.h - 2.0 * d,
        }
    }
}

/// One frame's draw list plus the input state it was built against.
pub(crate) struct Ui {
    pub(crate) verts: Vec<UiVertex>,
    mouse: [f32; 2],
    pub(crate) clicks: Vec<[f32; 2]>,
    pub(crate) pointer: bool,
    /// Font-size step used by widgets that draw their own text.
    pub(crate) font_level: usize,
}

impl Ui {
    pub(crate) fn new(mouse: [f32; 2]) -> Self {
        Self {
            verts: Vec::new(),
            mouse,
            clicks: Vec::new(),
            pointer: false,
            font_level: DEFAULT_LEVEL,
        }
    }

    pub(crate) fn hovered(&self, r: Rect) -> bool {
        r.contains(self.mouse)
    }

    pub(crate) fn take_click(&mut self, r: Rect) -> bool {
        match self.clicks.iter().position(|p| r.contains(*p)) {
            Some(i) => {
                self.clicks.remove(i);
                true
            }
            None => false,
        }
    }

    fn quad_rot(&mut self, center: [f32; 2], half: [f32; 2], angle: f32, color: [f32; 4]) {
        let (s, c) = angle.sin_cos();
        const CORNERS: [[f32; 2]; 6] = [
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 1.0],
            [-1.0, -1.0],
            [1.0, 1.0],
            [-1.0, 1.0],
        ];
        let uv = atlas::global().white_uv();
        for cnr in CORNERS {
            let lx = cnr[0] * half[0];
            let ly = cnr[1] * half[1];
            self.verts.push(UiVertex {
                pos: [center[0] + lx * c - ly * s, center[1] + lx * s + ly * c],
                uv,
                color,
            });
        }
    }

    pub(crate) fn rect(&mut self, r: Rect, color: [f32; 4]) {
        self.quad_rot(
            [r.x + r.w * 0.5, r.y + r.h * 0.5],
            [r.w * 0.5, r.h * 0.5],
            0.0,
            color,
        );
    }

    pub(crate) fn line(&mut self, a: [f32; 2], b: [f32; 2], thickness: f32, color: [f32; 4]) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-3 {
            return;
        }
        self.quad_rot(
            [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5],
            [len * 0.5, thickness * 0.5],
            dy.atan2(dx),
            color,
        );
    }

    /// Draws one glyph with its top-left placed from the pen position and baseline;
    /// returns the pen advance.
    fn glyph(&mut self, x: f32, baseline: f32, c: char, size: Size, color: [f32; 4]) -> f32 {
        let fa = atlas::global();
        if let Some(quad) = fa.quad(size, c) {
            let x0 = x + quad.left;
            let y0 = baseline - quad.top;
            const FRACS: [[f32; 2]; 6] = [
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ];
            for f in FRACS {
                self.verts.push(UiVertex {
                    pos: [x0 + f[0] * quad.w, y0 + f[1] * quad.h],
                    uv: fa.quad_uv(quad, f[0], f[1]),
                    color,
                });
            }
        }
        x + fa.advance(size, c)
    }

    /// Draws a string whose line box starts at (`x`, `y`); returns the pen end position.
    pub(crate) fn text_at(&mut self, x: f32, y: f32, s: &str, size: Size, color: [f32; 4]) -> f32 {
        let baseline = (y + atlas::global().size(size).ascent).round();
        let mut cx = x;
        for c in s.chars() {
            cx = self.glyph(cx, baseline, c, size, color);
        }
        cx
    }

    pub(crate) fn text_clipped(
        &mut self,
        x: f32,
        y: f32,
        s: &str,
        size: Size,
        color: [f32; 4],
        max_x: f32,
    ) -> f32 {
        let fa = atlas::global();
        let baseline = (y + fa.size(size).ascent).round();
        let mut cx = x;
        for c in s.chars() {
            if cx + fa.advance(size, c) > max_x {
                break;
            }
            cx = self.glyph(cx, baseline, c, size, color);
        }
        cx
    }
}

/// Vertical distance the rasterized line box occupies (ascent + descent + line gap).
pub(crate) fn line_height(size: Size) -> f32 {
    atlas::global().size(size).line_height
}

pub(crate) fn text_width(s: &str, size: Size) -> f32 {
    let fa = atlas::global();
    s.chars().map(|c| fa.advance(size, c)).sum()
}

pub(crate) fn fit_width(s: &str, size: Size, max_w: f32) -> f32 {
    let fa = atlas::global();
    let mut x = 0.0;
    for c in s.chars() {
        let adv = fa.advance(size, c);
        if x + adv > max_w {
            break;
        }
        x += adv;
    }
    x
}
