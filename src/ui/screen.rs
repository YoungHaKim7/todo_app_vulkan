//! The ToDo screen: builds one frame of vertices from app state and input.

use std::{path::Path, time::Instant};

use super::{
    Rect, Ui, fit_width, line_height, text_width,
    widgets::{button, caret_blinking, checkbox, delete_button},
};
use crate::font::Size;

use crate::{
    todos::Todos,
    ui::theme::{
        BTN_GHOST, BTN_PRIMARY, COL_ACCENT, COL_ACCENT_HOVER, COL_BORDER, COL_FIELD,
        COL_PLACEHOLDER, COL_ROW_ALT, COL_TEXT, COL_TEXT_DIM,
    },
};

pub(crate) fn draw_ui(todos: &mut Todos, save_path: &Path, ui: &mut Ui, w: f32, h: f32) {
    ui.pointer = false;
    ui.verts.clear();

    let pad = 26.0;
    let content_w = w - 2.0 * pad;

    ui.text_at(pad, pad, "ToDo", Size::Title, COL_ACCENT_HOVER);
    let counts = format!("{} open / {} done", todos.open_count(), todos.done_count());
    ui.text_at(
        w - pad - text_width(&counts, Size::Text),
        pad + 10.0,
        &counts,
        Size::Text,
        COL_TEXT_DIM,
    );

    let y0 = pad + line_height(Size::Title) + 16.0;
    let row_h = 38.0;
    let add_w = 88.0;
    let gap = 10.0;

    let field = Rect {
        x: pad,
        y: y0,
        w: content_w - add_w - gap,
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

    let ty = field.y + (field.h - line_height(Size::Text)) * 0.5;
    let tx = field.x + 12.0;
    let max_tx = field.x + field.w - 12.0;
    if todos.input.is_empty() && !todos.focused {
        ui.text_at(tx, ty, "What needs doing?", Size::Text, COL_PLACEHOLDER);
    } else {
        ui.text_clipped(tx, ty, &todos.input, Size::Text, COL_TEXT, max_tx);
    }
    if todos.focused && caret_blinking(todos.caret_since) {
        let caret_x = (tx + fit_width(&todos.input, Size::Text, max_tx - tx)).min(max_tx - 2.0);
        ui.rect(
            Rect {
                x: caret_x,
                y: ty - 3.0,
                w: 2.0,
                h: line_height(Size::Text) + 6.0,
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

    let list_top = y0 + row_h + 16.0;
    let list_bottom = h - 48.0;
    let pitch = 46.0;
    let item_h = 40.0;
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
                    w: content_w + 16.0,
                    h: item_h,
                },
                COL_ROW_ALT,
            );
        }

        let cb = Rect {
            x: pad + 2.0,
            y: ry + (item_h - 24.0) * 0.5,
            w: 24.0,
            h: 24.0,
        };
        if checkbox(ui, cb, todos.items[i].done) {
            todos.items[i].done = !todos.items[i].done;
            todos.save(save_path);
            interacted_elsewhere = true;
        }

        let text_x = cb.x + cb.w + 14.0;
        let del_btn = Rect {
            x: w - pad - 28.0,
            y: ry + (item_h - 28.0) * 0.5,
            w: 28.0,
            h: 28.0,
        };
        let text_col = if todos.items[i].done {
            COL_TEXT_DIM
        } else {
            COL_TEXT
        };
        let tw = ui.text_clipped(
            text_x,
            ry + (item_h - line_height(Size::Text)) * 0.5,
            &todos.items[i].text,
            Size::Text,
            text_col,
            del_btn.x - 14.0,
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
            (w - text_width(msg, Size::Text)) * 0.5,
            list_top + visible_h * 0.5 - line_height(Size::Text) * 0.5,
            msg,
            Size::Text,
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
    ui.text_at(pad, h - 36.0, hint, Size::Text, COL_TEXT_DIM);

    let done_n = todos.done_count();
    let clear_label = format!("Clear completed ({})", done_n);
    let clear_w = text_width(&clear_label, Size::Text) + 24.0;
    let clear_btn = Rect {
        x: w - pad - clear_w,
        y: h - 42.0,
        w: clear_w,
        h: 30.0,
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
}
