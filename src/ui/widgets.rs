//! Reusable interactive widgets: buttons, checkbox, delete affordance, caret blink.

use std::time::Instant;

use super::{Rect, Ui, line_height, text_width};
use crate::font::Size;
use crate::ui::theme::{
    BtnStyle, COL_BORDER, COL_CHECK, COL_DANGER_HOVER, COL_FIELD, COL_TEXT_DIM,
};

pub(crate) fn button(ui: &mut Ui, r: Rect, label: &str, style: &BtnStyle, enabled: bool) -> bool {
    let hot = enabled && ui.hovered(r);
    if hot {
        ui.pointer = true;
    }
    let clicked = enabled && ui.take_click(r);
    let bg = if !enabled {
        style.disabled
    } else if hot {
        style.hover
    } else {
        style.base
    };
    ui.rect(r, bg);
    let tw = text_width(label, Size::Text);
    let tc = if enabled { style.text } else { COL_TEXT_DIM };
    ui.text_at(
        r.x + (r.w - tw) * 0.5,
        r.y + (r.h - line_height(Size::Text)) * 0.5,
        label,
        Size::Text,
        tc,
    );
    clicked
}

pub(crate) fn checkbox(ui: &mut Ui, r: Rect, checked: bool) -> bool {
    let hot = ui.hovered(r);
    if hot {
        ui.pointer = true;
    }
    let clicked = ui.take_click(r);
    let active = checked || hot;
    ui.rect(r, if active { COL_CHECK } else { COL_BORDER });
    ui.rect(r.inset(2.0), COL_FIELD);
    if checked {
        let m = 0.28 * r.h;
        let p1 = [r.x + m, r.y + r.h * 0.55];
        let p2 = [r.x + r.w * 0.42, r.y + r.h - m * 0.7];
        let p3 = [r.x + r.w - m * 0.6, r.y + r.h * 0.25];
        ui.line(p1, p2, 2.5, COL_CHECK);
        ui.line(p2, p3, 2.5, COL_CHECK);
    }
    clicked
}

pub(crate) fn delete_button(ui: &mut Ui, r: Rect) -> bool {
    let hot = ui.hovered(r);
    if hot {
        ui.pointer = true;
    }
    let clicked = ui.take_click(r);
    if hot {
        ui.rect(r, [0.86, 0.37, 0.35, 0.18]);
    }
    let c = [r.x + r.w * 0.5, r.y + r.h * 0.5];
    let d = r.w * 0.26;
    let col = if hot { COL_DANGER_HOVER } else { COL_TEXT_DIM };
    ui.line([c[0] - d, c[1] - d], [c[0] + d, c[1] + d], 2.0, col);
    ui.line([c[0] - d, c[1] + d], [c[0] + d, c[1] - d], 2.0, col);
    clicked
}

pub(crate) fn caret_blinking(since: Instant) -> bool {
    (since.elapsed().as_millis() / 450).is_multiple_of(2)
}
