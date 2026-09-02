//! The ToDo screen: builds one frame of vertices from app state and input.

use std::{path::Path, time::Instant};

use super::{
    Rect, Ui, line_height, text_width,
    widgets::{
        button, caret_blinking, checkbox, delete_button, edit_button, gear_button, priority_button,
    },
};
use crate::font::{self, Size};

use crate::{
    input::Wrap,
    settings::Settings,
    todos::{Priority, Todos},
    ui::theme::{
        BTN_GHOST, BTN_PRIMARY, COL_ACCENT, COL_ACCENT_HOVER, COL_BORDER, COL_FIELD, COL_OVERLAY,
        COL_PANEL, COL_PLACEHOLDER, COL_PRIO_HIGH, COL_PRIO_LOW, COL_PRIO_MID, COL_ROW_ALT,
        COL_SELECTION, COL_TEXT, COL_TEXT_DIM,
    },
};

/// How far the layout stretches per font level, relative to the default level. Glyphs grow
/// a little faster than this so bigger text also packs slightly more densely.
const LAYOUT_SCALE: [f32; font::LEVELS] = [
    0.37, 0.44, 0.51, 0.58, 0.65, 0.72, 0.79, 0.86, 0.93, 1.0, 1.08, 1.16, 1.24, 1.32, 1.40, 1.48,
    1.56, 1.64, 1.72, 1.80,
];

/// How many wrapped lines the input field shows; anything past them runs off the
/// bottom of the field instead of scrolling.
const FIELD_MAX_LINES: usize = 2;

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

    let field_w = w - 2.0 * pad - add_w - gap;
    let tx = pad + 12.0 * s;
    let max_tx = pad + field_w - 12.0 * s;

    // The wrap of the text alone feeds the arrow keys (visual-line moves).
    todos.input.wrap = Wrap::new(&todos.input.text, max_tx - tx, text);

    // What the field draws: the text with any IME composition spliced in at the
    // caret, so the composition wraps together with the text it will join. Owned,
    // because selection edits below take the field mutably.
    let caret = todos.input.caret;
    let preedit_len = todos.preedit.as_deref().map_or(0, str::len);
    let mut display = todos.input.text.clone();
    if preedit_len > 0 {
        display.insert_str(caret, todos.preedit.as_deref().unwrap_or(""));
    }
    // Byte offsets map between the text and the display string by skipping the
    // spliced-in composition; inside it they clamp to the caret.
    let to_display = |o: usize| o + if o > caret { preedit_len } else { 0 };
    let to_text = |d: usize| {
        if d <= caret {
            d
        } else {
            d.saturating_sub(preedit_len).max(caret)
        }
    };

    // The field wraps instead of scrolling sideways: it grows to hold a second
    // line, and text past the shown lines runs off the bottom.
    let wrap = Wrap::new(&display, max_tx - tx, text);
    let shown = wrap.lines().min(FIELD_MAX_LINES);
    let content_h = shown as f32 * line_height(text);
    let field_h = row_h.max(content_h + 13.0 * s);
    let field = Rect {
        x: pad,
        y: y0,
        w: field_w,
        h: field_h,
    };
    todos.field_rect = field;
    let ty0 = field.y + (field_h - content_h) * 0.5;
    let line_y = |n: usize| ty0 + n as f32 * line_height(text);

    // A press that started in the field drives selection: its first frame plants the
    // caret with the anchor on top of it, every later frame drags the caret along
    // the mouse, extending the selection. The press maps to a line by its y, then
    // to the caret closest to its x on that line.
    if !modal_was_open && ui.mouse_down && field.contains(ui.press) {
        let p = if todos.field_drag { ui.mouse } else { ui.press };
        let n = (((p[1] - ty0) / line_height(text)).floor().max(0.0) as usize).min(shown - 1);
        let d = wrap.caret_at(&display, p[0] - tx, n);
        todos.input.caret = to_text(d);
        if !todos.field_drag {
            todos.field_drag = true;
            todos.input.anchor = todos.input.caret;
            todos.caret_since = Instant::now();
        }
    } else if !ui.mouse_down {
        todos.field_drag = false;
    }

    let was_focused = todos.focused;
    let field_click = ui.click_in(field);
    if let Some(pos) = field_click {
        todos.caret_since = Instant::now();
        // A quick second (or third) click in place selects a word (or everything);
        // it only counts when this click's press and release landed on the same spot.
        let no_drag = (ui.press[0] - pos[0]).abs() + (ui.press[1] - pos[1]).abs() < 8.0;
        let count = match todos.last_field_click {
            Some((at, p, n))
                if no_drag
                    && at.elapsed().as_millis() < 500
                    && (p[0] - pos[0]).abs() + (p[1] - pos[1]).abs() < 8.0 =>
            {
                n % 3 + 1
            }
            _ => 1,
        };
        todos.last_field_click = Some((Instant::now(), pos, count));
        match count {
            2 => todos.input.select_word_around(),
            3 => todos.input.select_all(),
            _ => {}
        }
    }
    let field_clicked = field_click.is_some();
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

    if todos.input.text.is_empty() && !todos.focused {
        ui.text_at(
            tx,
            field.y + (field_h - line_height(text)) * 0.5,
            "What needs doing?",
            text,
            COL_PLACEHOLDER,
        );
    } else {
        // Selection highlight under the text, one run per wrapped line.
        if let Some(sel) = todos.input.selection() {
            let (d0, d1) = (to_display(sel.start), to_display(sel.end));
            for n in 0..shown {
                let ls = wrap.line_start(n);
                let le = wrap.line_end(&display, n);
                let (a, b) = (d0.max(ls), d1.min(le));
                if b > a {
                    let x0 = tx + wrap.x_in_line(&display, a, n);
                    let x1 = (tx + wrap.x_in_line(&display, b, n)).min(max_tx);
                    if x1 > x0 {
                        ui.rect(
                            Rect {
                                x: x0,
                                y: line_y(n),
                                w: x1 - x0,
                                h: line_height(text),
                            },
                            COL_SELECTION,
                        );
                    }
                }
            }
        }

        // The text, one wrapped line at a time.
        for n in 0..shown {
            let seg = &display[wrap.line_start(n)..wrap.line_end(&display, n)];
            ui.text_clipped(tx, line_y(n), seg, text, COL_TEXT, max_tx);
        }

        // The IME composition (e.g. Hangul jamo merging into a syllable) renders at
        // the caret, underlined, until the IME commits it.
        if preedit_len > 0 {
            for n in 0..shown {
                let a = caret.max(wrap.line_start(n));
                let b = (caret + preedit_len).min(wrap.line_end(&display, n));
                if b > a {
                    let underline_y = line_y(n) + line_height(text) - 2.0 * s;
                    ui.line(
                        [tx + wrap.x_in_line(&display, a, n), underline_y],
                        [tx + wrap.x_in_line(&display, b, n), underline_y],
                        1.5,
                        COL_ACCENT,
                    );
                }
            }
        }
    }
    // The caret sits just before the composition; on a line past the shown ones it
    // is simply off screen.
    let caret_line = wrap.line_of(caret);
    let caret_x = (tx + wrap.x_at(&display, caret)).clamp(tx, max_tx - 2.0);
    if todos.focused {
        // Where the IME popup should appear, published even while the caret blinks off.
        todos.caret_area = Rect {
            x: caret_x,
            y: line_y(caret_line.min(shown - 1)),
            w: 2.0,
            h: line_height(text),
        };
        if caret_line < shown && caret_blinking(todos.caret_since) {
            ui.rect(
                Rect {
                    x: caret_x,
                    y: line_y(caret_line) - 3.0 * s,
                    w: 2.0,
                    h: line_height(text) + 6.0 * s,
                },
                COL_ACCENT_HOVER,
            );
        }
    }

    let add_btn = Rect {
        x: w - pad - add_w,
        y: y0,
        w: add_w,
        h: field_h,
    };
    // While a task is being edited the same button commits the edit.
    let add_label = if todos.editing.is_some() {
        "Save"
    } else {
        "Add"
    };
    let add_clicked = button(
        ui,
        add_btn,
        add_label,
        &BTN_PRIMARY,
        !todos.input.text.trim().is_empty(),
    );
    if add_clicked {
        todos.add_task(save_path);
    }

    let list_top = y0 + field_h + 16.0 * s;
    let list_bottom = h - 48.0 * s;
    let pitch = 46.0 * s;
    let item_h = 40.0 * s;
    let visible_h = (list_bottom - list_top).max(0.0);

    todos.max_scroll = (todos.items.len() as f32 * pitch - visible_h).max(0.0);
    todos.scroll = todos.scroll.clamp(0.0, todos.max_scroll);

    let first = (todos.scroll / pitch).floor() as usize;
    let visible = (visible_h / pitch).ceil() as usize + 1;

    let mut interacted_elsewhere = false;
    // A pencil click starts an edit, which lives in the input field — so unlike the
    // other row clicks it must leave the field focused.
    let mut edit_started = false;

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

        // Priority stripe before the checkbox — red = emergency, yellow = next,
        // gray = general. The thin bar carries a wide row-height hit rect so one
        // click lands; a click cycles the priority and the row re-sorts.
        let stripe_hit = Rect {
            x: pad,
            y: ry,
            w: 13.0 * s,
            h: item_h,
        };
        let stripe = Rect {
            x: pad + 2.0 * s,
            y: ry + 5.0 * s,
            w: 5.0 * s,
            h: item_h - 10.0 * s,
        };
        let stripe_clicked = priority_button(
            ui,
            stripe_hit,
            stripe,
            priority_color(todos.items[i].priority),
        );

        let cb = Rect {
            x: pad + 15.0 * s,
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
        // The pencil sits just in front of the X; the row's text clips short of it.
        let edit_btn = Rect {
            x: del_btn.x - 34.0 * s,
            y: del_btn.y,
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
            edit_btn.x - 14.0 * s,
        );
        if todos.items[i].done && tw > text_x {
            ui.line(
                [text_x, ry + item_h * 0.5],
                [tw, ry + item_h * 0.5],
                2.0,
                COL_TEXT_DIM,
            );
        }
        if edit_button(ui, edit_btn) {
            // The task lifts into the input field and its row leaves the list until
            // the edit commits or cancels, so the loop stops like a delete's.
            todos.begin_edit(i);
            edit_started = true;
            break;
        }
        if delete_button(ui, del_btn) {
            todos.items.remove(i);
            todos.save(save_path);
            interacted_elsewhere = true;
            break;
        }
        // Handled last so the row above is fully drawn from pre-cycle state; the
        // re-sort then re-draws every row fresh on the next frame.
        if stripe_clicked {
            todos.cycle_priority(i, save_path);
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

    let done_n = todos.done_count();
    let clear_label = format!("Clear completed ({})", done_n);
    let clear_w = text_width(&clear_label, text) + 24.0 * s;
    let clear_btn = Rect {
        x: w - pad - clear_w,
        y: h - 42.0 * s,
        w: clear_w,
        h: 30.0 * s,
    };

    // The hint gives way to the clear button: at large font sizes the two no longer
    // fit side by side, so it clips short of the button instead of running under it.
    let hint = "Enter: add/save · stripe: priority · Esc: quit";
    ui.text_clipped(
        pad,
        h - 36.0 * s,
        hint,
        text,
        COL_TEXT_DIM,
        clear_btn.x - 12.0 * s,
    );

    if button(ui, clear_btn, &clear_label, &BTN_GHOST, done_n > 0) {
        todos.items.retain(|t| !t.done);
        todos.save(save_path);
        interacted_elsewhere = true;
    }

    if !ui.clicks.is_empty() {
        // A release landing outside the field after a press inside it (a selection
        // drag that ran past the edge) keeps focus on the field; only clicks that
        // started elsewhere move focus away from it.
        if !field.contains(ui.press) {
            interacted_elsewhere = true;
        }
        ui.clicks.clear();
    }
    let focused =
        field_clicked || add_clicked || edit_started || (was_focused && !interacted_elsewhere);
    if focused {
        todos.focused = true;
    } else if todos.focused {
        // Losing focus collapses the selection so no highlight is left that
        // Del/Backspace can no longer erase.
        todos.blur();
    }
    if todos.focused != was_focused {
        todos.caret_since = Instant::now();
    }

    if modal_was_open {
        // The settings window owns input while open; nothing underneath keeps focus or
        // shows a pointer cursor through the dimmer.
        todos.blur();
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

/// Stripe color for a priority: red = emergency, yellow = next up, gray = general.
fn priority_color(p: Priority) -> [f32; 4] {
    match p {
        Priority::High => COL_PRIO_HIGH,
        Priority::Mid => COL_PRIO_MID,
        Priority::Low => COL_PRIO_LOW,
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
