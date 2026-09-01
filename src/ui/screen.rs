//! The ToDo screen: builds one frame of vertices from app state and input.

use std::{path::Path, time::Instant};

use super::{
    Rect, Ui, fit_width, line_height, text_width,
    widgets::{button, caret_blinking, checkbox, delete_button, gear_button},
};
use crate::font::{self, Size};

use crate::{
    settings::Settings,
    todos::Todos,
    ui::theme::{
        BTN_GHOST, BTN_PRIMARY, COL_ACCENT, COL_ACCENT_HOVER, COL_BORDER, COL_FIELD, COL_OVERLAY,
        COL_PANEL, COL_PLACEHOLDER, COL_ROW_ALT, COL_TEXT, COL_TEXT_DIM,
    },
};

/// How far the layout stretches per font level, relative to the default level. Glyphs grow
/// a little faster than this so bigger text also packs slightly more densely.
const LAYOUT_SCALE: [f32; font::LEVELS] = [0.86, 0.93, 1.0, 1.16, 1.32];

pub(crate) fn draw_ui(
    todos: &mut Todos,
    settings: &mut Settings,
    save_path: &Path,
    settings_path: &Path,
    ui: &mut Ui,
    w: f32,
    h: f32,
) {
    ui.pointer = false;
    ui.verts.clear();
    ui.font_level = settings.font_level;

    let s = LAYOUT_SCALE[settings.font_level.min(LAYOUT_SCALE.len() - 1)];
    let text = Size::text(settings.font_level);
    let title = Size::title(settings.font_level);

    // While the settings window is open it owns all input: hold this frame's clicks back
    // from the main UI and hand them to the window after it is drawn.
    let modal_was_open = settings.open;
    let held_clicks = if modal_was_open {
        std::mem::take(&mut ui.clicks)
    } else {
        Vec::new()
    };

    let pad = 26.0 * s;

    // Header: settings gear at the top left, title right of it, counts at the top right.
    let gear_side = (line_height(title) * 0.95).round();
    let gear = Rect {
        x: pad,
        y: pad + (line_height(title) - gear_side) * 0.5,
        w: gear_side,
        h: gear_side,
    };
    if gear_button(ui, gear) {
        settings.open = true;
    }
    ui.text_at(
        gear.x + gear.w + 16.0 * s,
        pad,
        "ToDo",
        title,
        COL_ACCENT_HOVER,
    );
    let counts = format!("{} open / {} done", todos.open_count(), todos.done_count());
    ui.text_at(
        w - pad - text_width(&counts, text),
        pad + 10.0 * s,
        &counts,
        text,
        COL_TEXT_DIM,
    );

    let y0 = pad + line_height(title) + 16.0 * s;
    let row_h = 38.0 * s;
    let add_w = 88.0 * s;
    let gap = 10.0 * s;

    let field = Rect {
        x: pad,
        y: y0,
        w: w - 2.0 * pad - add_w - gap,
        h: row_h,
    };
    let was_focused = todos.focused;
    let field_clicked = ui.take_click(field);
    if field_clicked {
        todos.caret_since = Instant::now();
    }
    if ui.hovered(field) {
        ui.pointer = true;
    }
    ui.rect(
        field,
        if todos.focused {
            COL_ACCENT
        } else {
            COL_BORDER
        },
    );
    ui.rect(field.inset(1.5), COL_FIELD);

    let ty = field.y + (field.h - line_height(text)) * 0.5;
    let tx = field.x + 12.0 * s;
    let max_tx = field.x + field.w - 12.0 * s;
    if todos.input.is_empty() && !todos.focused {
        ui.text_at(tx, ty, "What needs doing?", text, COL_PLACEHOLDER);
    } else {
        ui.text_clipped(tx, ty, &todos.input, text, COL_TEXT, max_tx);
    }
    if todos.focused && caret_blinking(todos.caret_since) {
        let caret_x = (tx + fit_width(&todos.input, text, max_tx - tx)).min(max_tx - 2.0);
        ui.rect(
            Rect {
                x: caret_x,
                y: ty - 3.0 * s,
                w: 2.0,
                h: line_height(text) + 6.0 * s,
            },
            COL_ACCENT_HOVER,
        );
    }

    let add_btn = Rect {
        x: w - pad - add_w,
        y: y0,
        w: add_w,
        h: row_h,
    };
    let add_clicked = button(
        ui,
        add_btn,
        "Add",
        &BTN_PRIMARY,
        !todos.input.trim().is_empty(),
    );
    if add_clicked {
        todos.add_task(save_path);
    }

    let list_top = y0 + row_h + 16.0 * s;
    let list_bottom = h - 48.0 * s;
    let pitch = 46.0 * s;
    let item_h = 40.0 * s;
    let visible_h = (list_bottom - list_top).max(0.0);

    todos.max_scroll = (todos.items.len() as f32 * pitch - visible_h).max(0.0);
    todos.scroll = todos.scroll.clamp(0.0, todos.max_scroll);

    let first = (todos.scroll / pitch).floor() as usize;
    let visible = (visible_h / pitch).ceil() as usize + 1;

    let mut interacted_elsewhere = false;

    for i in first..todos.items.len().min(first + visible) {
        let ry = list_top + i as f32 * pitch - todos.scroll;
        let top = ry;
        let bottom = ry + item_h;
        if top < list_top || bottom > list_bottom {
            continue;
        }
        if i % 2 == 1 {
            ui.rect(
                Rect {
                    x: pad - 8.0,
                    y: ry,
                    w: w - 2.0 * pad + 16.0,
                    h: item_h,
                },
                COL_ROW_ALT,
            );
        }

        let cb = Rect {
            x: pad + 2.0 * s,
            y: ry + (item_h - 24.0 * s) * 0.5,
            w: 24.0 * s,
            h: 24.0 * s,
        };
        if checkbox(ui, cb, todos.items[i].done) {
            todos.items[i].done = !todos.items[i].done;
            todos.save(save_path);
            interacted_elsewhere = true;
        }

        let text_x = cb.x + cb.w + 14.0 * s;
        let del_btn = Rect {
            x: w - pad - 28.0 * s,
            y: ry + (item_h - 28.0 * s) * 0.5,
            w: 28.0 * s,
            h: 28.0 * s,
        };
        let text_col = if todos.items[i].done {
            COL_TEXT_DIM
        } else {
            COL_TEXT
        };
        let tw = ui.text_clipped(
            text_x,
            ry + (item_h - line_height(text)) * 0.5,
            &todos.items[i].text,
            text,
            text_col,
            del_btn.x - 14.0 * s,
        );
        if todos.items[i].done && tw > text_x {
            ui.line(
                [text_x, ry + item_h * 0.5],
                [tw, ry + item_h * 0.5],
                2.0,
                COL_TEXT_DIM,
            );
        }
        if delete_button(ui, del_btn) {
            todos.items.remove(i);
            todos.save(save_path);
            interacted_elsewhere = true;
            break;
        }
    }

    if todos.items.is_empty() {
        let msg = "No tasks yet. Type above and press Enter.";
        ui.text_at(
            (w - text_width(msg, text)) * 0.5,
            list_top + visible_h * 0.5 - line_height(text) * 0.5,
            msg,
            text,
            COL_PLACEHOLDER,
        );
    }

    if todos.max_scroll > 0.0 {
        let track = Rect {
            x: w - 5.0,
            y: list_top,
            w: 3.0,
            h: visible_h,
        };
        ui.rect(track, COL_ROW_ALT);
        let thumb_h = (visible_h * visible_h / (visible_h + todos.max_scroll)).max(24.0);
        let thumb_y = track.y + (track.h - thumb_h) * (todos.scroll / todos.max_scroll.max(1e-3));
        ui.rect(
            Rect {
                x: track.x,
                y: thumb_y,
                w: track.w,
                h: thumb_h,
            },
            COL_BORDER,
        );
    }

    let hint = "Enter: add · Esc: quit";
    ui.text_at(pad, h - 36.0 * s, hint, text, COL_TEXT_DIM);

    let done_n = todos.done_count();
    let clear_label = format!("Clear completed ({})", done_n);
    let clear_w = text_width(&clear_label, text) + 24.0 * s;
    let clear_btn = Rect {
        x: w - pad - clear_w,
        y: h - 42.0 * s,
        w: clear_w,
        h: 30.0 * s,
    };
    if button(ui, clear_btn, &clear_label, &BTN_GHOST, done_n > 0) {
        todos.items.retain(|t| !t.done);
        todos.save(save_path);
        interacted_elsewhere = true;
    }

    if !ui.clicks.is_empty() {
        interacted_elsewhere = true;
        ui.clicks.clear();
    }
    todos.focused = field_clicked || add_clicked || (was_focused && !interacted_elsewhere);
    if todos.focused != was_focused {
        todos.caret_since = Instant::now();
    }

    if modal_was_open {
        // The settings window owns input while open; nothing underneath keeps focus or
        // shows a pointer cursor through the dimmer.
        todos.focused = false;
        ui.pointer = false;

        let panel = settings_panel(w, h, s);
        if held_clicks.iter().any(|p| !panel.contains(*p)) {
            settings.open = false;
        } else {
            ui.clicks = held_clicks;
            draw_settings_window(settings, settings_path, ui, w, h, &panel, s, text, title);
        }
    }
}

/// Centered settings window rect for the current layout scale.
fn settings_panel(w: f32, h: f32, s: f32) -> Rect {
    let pw = (380.0 * s).min(w - 24.0);
    let ph = (210.0 * s).min(h - 24.0);
    Rect {
        x: (w - pw) * 0.5,
        y: (h - ph) * 0.5,
        w: pw,
        h: ph,
    }
}

/// The settings window: a dimmer over the main UI, then a bordered panel with a font-size
/// stepper and a close button. Clicks were pre-filtered by the caller: only clicks inside
/// `panel` reach this function.
fn draw_settings_window(
    settings: &mut Settings,
    settings_path: &Path,
    ui: &mut Ui,
    w: f32,
    h: f32,
    panel: &Rect,
    s: f32,
    text: Size,
    title: Size,
) {
    // Dim the whole screen behind the window, then the window itself.
    ui.rect(
        Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        },
        COL_OVERLAY,
    );
    ui.rect(*panel, COL_BORDER);
    ui.rect(panel.inset(1.5), COL_PANEL);

    let pad_in = 24.0 * s;
    let divider_y = panel.y + 20.0 * s + line_height(title) + 12.0 * s;
    ui.text_at(
        panel.x + pad_in,
        panel.y + 20.0 * s,
        "Settings",
        title,
        COL_TEXT,
    );
    ui.line(
        [panel.x, divider_y],
        [panel.x + panel.w, divider_y],
        1.0,
        COL_BORDER,
    );

    // Font size row: label on the left, stepper ([−] 20 px [+]) on the right.
    let row_y = divider_y + 18.0 * s;
    let row_h = 36.0 * s;
    let btn_side = 36.0 * s;
    let step = 10.0 * s;
    let px_label = format!("{} px", text.px() as i32);
    let px_w = text_width(&px_label, text);
    let mut cx = panel.x + panel.w - pad_in - 2.0 * btn_side - px_w - 2.0 * step;
    ui.text_at(
        panel.x + pad_in,
        row_y + (row_h - line_height(text)) * 0.5,
        "Font size",
        text,
        COL_TEXT,
    );

    let minus = Rect {
        x: cx,
        y: row_y,
        w: btn_side,
        h: btn_side,
    };
    if button(ui, minus, "−", &BTN_GHOST, settings.font_level > 0) {
        settings.set_font_level(settings.font_level - 1, settings_path);
    }
    cx += btn_side + step;
    ui.text_at(
        cx,
        row_y + (row_h - line_height(text)) * 0.5,
        &px_label,
        text,
        COL_TEXT_DIM,
    );
    cx += px_w + step;
    let plus = Rect {
        x: cx,
        y: row_y,
        w: btn_side,
        h: btn_side,
    };
    if button(
        ui,
        plus,
        "+",
        &BTN_PRIMARY,
        settings.font_level + 1 < font::LEVELS,
    ) {
        settings.set_font_level(settings.font_level + 1, settings_path);
    }

    let close_label = "Close";
    let close_w = text_width(close_label, text) + 24.0 * s;
    let close = Rect {
        x: panel.x + panel.w - pad_in - close_w,
        y: panel.y + panel.h - 16.0 * s - 30.0 * s,
        w: close_w,
        h: 30.0 * s,
    };
    if button(ui, close, close_label, &BTN_GHOST, true) {
        settings.open = false;
    }
}
