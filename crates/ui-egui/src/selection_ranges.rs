//! Per-candidate selection handles. Preview is ephemeral; release enters native project history once.
use crate::{FilmcraftApp, dock::PanelKind, theme::Tokens};
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, pos2};
use filmcraft_engine::source_monitor::{RangeEdge, adjust_range};
use filmcraft_project::{ItemId, SelectionRange};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use serde_json::json;

#[derive(Clone)]
struct Selected {
    item: ItemId,
    range_id: String,
    category: String,
    incarnation: u64,
}
#[derive(Clone)]
struct Drag {
    item: ItemId,
    category: String,
    range: Option<SelectionRange>,
    edge: RangeEdge,
    original: TimeRange,
    preview: TimeRange,
    bounds: TimeRange,
    rate: FrameRate,
    pointer_x: f32,
    incarnation: u64,
    project_revision: u64,
}
#[derive(Default)]
pub struct State {
    selected: Option<Selected>,
    drag: Option<Drag>,
    area: Option<Rect>,
    focused: bool,
    active_edge: Option<RangeEdge>,
    cancelled: bool,
}
fn key(item: ItemId, id: &str, part: &str) -> egui::Id {
    egui::Id::new(("selection-range", item.0, id, part))
}
fn event_span(app: &FilmcraftApp, item: ItemId, range: &SelectionRange) -> Option<TimeRange> {
    if range.manual {
        return None;
    }
    let result = &app.session.project.recognition.get(&item)?["result"];
    if range.result_id.as_deref() != result["result_id"].as_str() {
        return None;
    }
    let events =
        result.pointer(&format!("/categories/{}/events", range.category)).or_else(|| (range.category == "gunshot").then_some(&result["events"]))?.as_array()?;
    let (_, event) = events.iter().enumerate().find(|(index, e)| {
        let id = e["event_id"].as_str().map(str::to_string).unwrap_or_else(|| format!("e{}", index + 1));
        range.event_ids.contains(&id)
    })?;
    Some(TimeRange::from_bounds(Tick::from_seconds_f64(event["start_s"].as_f64()?), Tick::from_seconds_f64(event["end_s"].as_f64()?)))
}
fn seek(app: &mut FilmcraftApp, item: ItemId, time: Tick) {
    app.stop_source();
    if app.session.state.source_item != Some(item) {
        let _ = app.session.execute("source.open", json!({"item":item.0}));
    }
    if app.session.state.source_playhead != time {
        let _ = app.session.execute("source.setPlayhead", json!({"time":time.0}));
    }
}
fn focus(app: &mut FilmcraftApp, ctx: &egui::Context, item: ItemId, range: &SelectionRange, part: &str) {
    app.selection_ranges.selected =
        Some(Selected { item, range_id: range.range_id.clone(), category: range.category.clone(), incarnation: app.session.recognition_jobs.incarnation });
    app.selection_ranges.focused = true;
    app.ui.focused = PanelKind::Timeline;
    ctx.memory_mut(|m| m.request_focus(key(item, &range.range_id, part)));
}
fn commit(app: &mut FilmcraftApp, drag: Drag) {
    if drag.incarnation != app.session.recognition_jobs.incarnation || drag.project_revision != app.session.revision || drag.preview.duration <= Tick::ZERO {
        return;
    }
    let mut params = json!({"item":drag.item.0,"start":drag.preview.start.0,"duration":drag.preview.duration.0,"projectToken":drag.incarnation});
    let command = if let Some(range) = &drag.range {
        if drag.preview == drag.original {
            return;
        }
        params["rangeId"] = json!(range.range_id);
        params["expectedRevision"] = json!(range.revision);
        "selection.setRange"
    } else {
        params["category"] = json!(drag.category);
        "selection.addRange"
    };
    match app.session.execute(command, params) {
        Ok(value) => {
            if let Some(id) = value["rangeId"].as_str() {
                app.ui.status = format!("待剪范围 {:.3}–{:.3} 秒（原片）", drag.preview.start.seconds(), drag.preview.end().seconds());
                app.selection_ranges.selected = Some(Selected { item: drag.item, range_id: id.into(), category: drag.category, incarnation: drag.incarnation });
            }
        }
        Err(e) => app.ui.status = e.to_string(),
    }
}
/// Called before application shortcuts. It owns keys only while a selection widget has focus.
pub fn handle_keys(app: &mut FilmcraftApp, ctx: &egui::Context) -> bool {
    if crate::panels::source_drag_in_context(ctx).is_some_and(|source| source.selection.is_some())
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        crate::panels::cancel_drag_context(ctx);
        app.selection_ranges.cancelled = true;
        return true;
    }
    if app.dialog.is_some() {
        app.selection_ranges.drag = None;
        return false;
    }
    if app.selection_ranges.drag.as_ref().is_some_and(|d| {
        d.incarnation != app.session.recognition_jobs.incarnation
            || d.project_revision != app.session.revision
            || app.session.state.source_item.is_some_and(|id| id != d.item)
    }) {
        app.selection_ranges.drag = None;
        app.selection_ranges.cancelled = true;
    }
    if app.selection_ranges.selected.as_ref().is_some_and(|s| s.incarnation != app.session.recognition_jobs.incarnation) {
        app.selection_ranges = State::default();
        return false;
    }
    if app.selection_ranges.selected.as_ref().is_some_and(|selected| app.session.state.source_item.is_some_and(|id| id != selected.item)) {
        app.selection_ranges = State::default();
        return false;
    }
    if ctx.input(|i| i.pointer.primary_pressed()) && ctx.pointer_hover_pos().is_some_and(|p| !app.selection_ranges.area.is_some_and(|r| r.contains(p))) {
        if app.selection_ranges.focused {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id)
                }
            });
        }
        app.selection_ranges.focused = false;
    }
    if app.selection_ranges.drag.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.selection_ranges.drag = None;
        app.selection_ranges.cancelled = true;
        return true;
    }
    if !app.selection_ranges.focused || app.ui.focused != PanelKind::Timeline || ctx.text_edit_focused() {
        return false;
    }
    let Some(selected) = app.selection_ranges.selected.clone() else { return false };
    let owns_focus =
        ctx.memory(|m| m.focused()).is_some_and(|id| ["body", "start", "end"].iter().any(|part| key(selected.item, &selected.range_id, part) == id));
    if !owns_focus {
        app.selection_ranges.focused = false;
        return false;
    }
    let mods = ctx.input(|i| i.modifiers);
    if mods.shift && !mods.command && !mods.ctrl && !mods.alt && ctx.input_mut(|i| i.consume_key(mods, egui::Key::Enter)) {
        if app.selection_ranges.drag.is_none() && !ctx.input(|i| i.pointer.any_down()) {
            if let Err(e) = crate::selection_drop::append(app, ctx, selected.item, &selected.range_id) {
                app.ui.status = e;
            }
        }
        return true;
    }
    if mods.command && ctx.input_mut(|i| i.consume_key(mods, egui::Key::Z)) {
        app.selection_ranges.drag = None;
        if crate::panels::source_drag_in_context(ctx).is_some_and(|s| s.selection.is_some()) {
            crate::panels::cancel_drag_context(ctx);
        }
        if let Err(e) = app.session.execute(if mods.shift { "edit.redo" } else { "edit.undo" }, json!({})) {
            app.ui.status = e.to_string();
        }
        return true;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.selection_ranges.focused = false;
        app.selection_ranges.selected = None;
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id)
            }
        });
        return true;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
        if let Err(e) = app.toggle_source_play() {
            app.ui.status = e;
        }
        return true;
    }
    if mods.command || mods.ctrl || mods.alt {
        return false;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
        return true;
    }
    let Ok(ranges) = filmcraft_engine::selection::ranges(&app.session, selected.item) else { return false };
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
        let matching = ranges.iter().filter(|r| r.category == selected.category).collect::<Vec<_>>();
        if let Some(index) = matching.iter().position(|r| r.range_id == selected.range_id) {
            let next = if mods.shift { (index + matching.len() - 1) % matching.len() } else { (index + 1) % matching.len() };
            focus(app, ctx, selected.item, matching[next], "body");
            seek(app, selected.item, matching[next].start);
        }
        return true;
    }
    let Some(range) = ranges.into_iter().find(|r| r.range_id == selected.range_id) else { return false };
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket)) {
        app.selection_ranges.active_edge = Some(RangeEdge::In);
        seek(app, selected.item, range.start);
        return true;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket)) {
        app.selection_ranges.active_edge = Some(RangeEdge::Out);
        if let Some(item) = app.session.project.item(selected.item) {
            let time = range.range().end() - item.frame_rate().frame_duration();
            seek(app, selected.item, time);
        }
        return true;
    }
    let delta = if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)) {
        -1
    } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)) {
        1
    } else {
        return false;
    };
    let Some(item) = app.session.project.item(selected.item) else { return true };
    let rate = item.frame_rate();
    let bounds = TimeRange::new(Tick::ZERO, rate.snap(item.duration()));
    let edge = app.selection_ranges.active_edge.unwrap_or(RangeEdge::In);
    let delta = rate.tick_of(delta * if mods.shift { 10 } else { 1 });
    if let Ok(preview) = adjust_range(bounds, range.range(), rate, edge, delta) {
        let drag = Drag {
            item: selected.item,
            category: range.category.clone(),
            range: Some(range.clone()),
            edge,
            original: range.range(),
            preview,
            bounds,
            rate,
            pointer_x: 0.0,
            incarnation: selected.incarnation,
            project_revision: app.session.revision,
        };
        let at = if edge == RangeEdge::Out { preview.end() - rate.frame_duration() } else { preview.start };
        commit(app, drag);
        seek(app, selected.item, at);
    }
    true
}

/// One category row. Body and boundary hit regions are disjoint; model event painting remains separate.
pub fn begin_frame(app: &mut FilmcraftApp) {
    app.selection_ranges.area = None;
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, item: ItemId, category: &str, row: Rect, x0: f32, pps: f64, scroll: f64) {
    let ctx = ui.ctx().clone();
    app.selection_ranges.area = Some(app.selection_ranges.area.map_or(row, |r| r.union(row)));
    if !ctx.input(|i| i.pointer.any_down()) {
        app.selection_ranges.cancelled = false;
    }
    if let Some(d) = &app.selection_ranges.drag {
        if d.item != item || d.incarnation != app.session.recognition_jobs.incarnation || d.project_revision != app.session.revision {
            app.selection_ranges.drag = None;
        }
    }
    let Some(source) = app.session.project.item(item) else { return };
    let rate = source.frame_rate();
    let bounds = TimeRange::new(Tick::ZERO, rate.snap(source.duration()));
    if bounds.duration <= Tick::ZERO {
        return;
    }
    let Ok(mut ranges) = filmcraft_engine::selection::ranges(&app.session, item) else { return };
    let selected = app.selection_ranges.selected.as_ref().map(|s| s.range_id.clone());
    let hovered_event = ctx.pointer_hover_pos().filter(|p| row.contains(*p)).and_then(|pointer| {
        let time = Tick::from_seconds_f64(scroll + (pointer.x - x0) as f64 / pps);
        ranges.iter().filter(|r| r.category == category).find(|r| event_span(app, item, r).is_some_and(|span| span.contains(time))).map(|r| r.range_id.clone())
    });
    ranges.sort_by_key(|r| selected.as_ref() == Some(&r.range_id));
    let x = |t: Tick| x0 + ((t.seconds() - scroll) * pps) as f32;
    let blank = ui.interact(row, egui::Id::new(("selection-create", item.0, category)), Sense::click_and_drag());
    app.auto.add(&format!("selection.{category}.create"), row, "在空白处划选补片；Escape 取消");
    if blank.drag_started()
        && !app.selection_ranges.cancelled
        && let Some(pointer) = ctx.input(|i| i.pointer.press_origin())
    {
        let anchor = rate.snap_nearest(Tick::from_seconds_f64((scroll + (pointer.x - x0) as f64 / pps).clamp(0.0, bounds.end().seconds())));
        app.selection_ranges.drag = Some(Drag {
            item,
            category: category.into(),
            range: None,
            edge: RangeEdge::Move,
            original: TimeRange::new(anchor, Tick::ZERO),
            preview: TimeRange::new(anchor, Tick::ZERO),
            bounds,
            rate,
            pointer_x: pointer.x,
            incarnation: app.session.recognition_jobs.incarnation,
            project_revision: app.session.revision,
        });
        app.selection_ranges.focused = true;
        app.ui.focused = PanelKind::Timeline;
        app.stop_source();
    }
    for range in ranges.iter().filter(|r| r.category == category) {
        let mut span = range.range();
        if let Some(d) = &app.selection_ranges.drag
            && d.range.as_ref().is_some_and(|r| r.range_id == range.range_id)
        {
            span = d.preview;
        }
        let a = x(span.start);
        let b = x(span.end());
        let band = Rect::from_min_max(pos2(a.max(row.left()), row.top() + 1.0), pos2(b.min(row.right()), row.bottom() - 1.0));
        if !band.is_positive() {
            continue;
        }
        let active = selected.as_ref() == Some(&range.range_id);
        let color = if range.manual { Color32::from_rgba_unmultiplied(90, 180, 145, 60) } else { Color32::from_rgba_unmultiplied(70, 155, 230, 40) };
        ui.painter().rect_filled(band, 2.0, color);
        if active {
            ui.painter().rect_stroke(band, 2.0, Stroke::new(1.0, Color32::LIGHT_BLUE), StrokeKind::Inside);
        }
        let middle = (a + b) * 0.5;
        let core = event_span(app, item, range);
        let (body_start, body_end) =
            if active || range.manual { (a + 6.0, b - 6.0) } else { core.map(|span| (x(span.start), x(span.end()))).unwrap_or((a + 6.0, b - 6.0)) };
        let (body_start, body_end) = if active || range.manual { (body_start.min(middle), body_end.max(middle)) } else { (body_start, body_end) };
        let body = Rect::from_min_max(pos2(body_start.max(row.left()), row.top()), pos2(body_end.min(row.right()), row.bottom()));
        let mut body_hovered = false;
        if body.is_positive() {
            let sense = if active && hovered_event.as_ref().is_some_and(|id| id != &range.range_id) { Sense::hover() } else { Sense::click_and_drag() };
            let response = ui.interact(body, key(item, &range.range_id, "body"), sense);
            body_hovered = response.hovered();
            app.auto.add(&format!("selection.{}.{}.body", category, range.range_id), body, "待剪范围主体；[ / ] 选择边界，方向键调整，Tab 切换，Escape 取消");
            if response.clicked() || response.drag_started() || response.has_focus() {
                focus(app, &ctx, item, range, "body");
            }
            if response.drag_started() && app.selection_ranges.drag.is_none() {
                if let Err(e) = crate::selection_drop::begin(app, ui, item, &range.range_id) {
                    app.ui.status = e;
                }
            }
            let usage = crate::selection_drop::usage_label(app, item, span);
            if active || body_hovered {
                ui.painter().text(band.left_top() + egui::vec2(3.0, 2.0), egui::Align2::LEFT_TOP, &usage, Tokens::ui(9.0), Color32::WHITE);
            }
            if response.clicked() {
                let time = core.map_or(span.start, |event| event.start);
                seek(app, item, time);
            }
            response.on_hover_text(format!(
                "{}待剪范围 {:.3}–{:.3} 秒\n[ 起点 / ] 终点 · ←/→ 一帧 · Shift 十帧 · Tab 下一范围 · 空格原片预览 · Shift+Enter 追加 · {usage}",
                if range.manual { "手动" } else { "" },
                span.start.seconds(),
                span.end().seconds()
            ));
        }
        if !active && !body_hovered {
            continue;
        }
        for (part, edge, at) in [("start", RangeEdge::In, a), ("end", RangeEdge::Out, b)] {
            if at < row.left() || at > row.right() {
                continue;
            }
            let left = if edge == RangeEdge::Out { (at - 6.0).max(middle) } else { at - 6.0 };
            let right = if edge == RangeEdge::In { (at + 6.0).min(middle) } else { at + 6.0 };
            let handle = Rect::from_min_max(pos2(left.max(row.left()), row.top()), pos2(right.min(row.right()), row.bottom()));
            let response = ui.interact(handle, key(item, &range.range_id, part), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            app.auto.add(&format!("selection.{}.{}.{}", category, range.range_id, part), handle, if part == "start" { "调整待剪起点" } else { "调整待剪终点" });
            if active || response.hovered() || response.dragged() {
                ui.painter().line_segment([pos2(at, row.top() + 3.0), pos2(at, row.bottom() - 3.0)], Stroke::new(2.0, Color32::LIGHT_BLUE));
            }
            if response.clicked() || response.drag_started() {
                focus(app, &ctx, item, range, part);
                app.selection_ranges.active_edge = Some(edge);
            }
            if response.clicked() {
                seek(app, item, if edge == RangeEdge::Out { span.end() - rate.frame_duration() } else { span.start });
            }
            if response.drag_started()
                && !app.selection_ranges.cancelled
                && let Some(pointer) = ctx.input(|i| i.pointer.press_origin())
            {
                app.stop_source();
                app.selection_ranges.drag = Some(Drag {
                    item,
                    category: category.into(),
                    range: Some(range.clone()),
                    edge,
                    original: range.range(),
                    preview: range.range(),
                    bounds,
                    rate,
                    pointer_x: pointer.x,
                    incarnation: app.session.recognition_jobs.incarnation,
                    project_revision: app.session.revision,
                });
            }
        }
    }
    if let Some(mut drag) = app.selection_ranges.drag.clone().filter(|d| d.item == item && d.category == category) {
        if let Some(pointer) = ctx.input(|i| i.pointer.interact_pos()) {
            let delta = Tick::from_seconds_f64((pointer.x - drag.pointer_x) as f64 / pps);
            if drag.range.is_some() {
                if let Ok(preview) = adjust_range(drag.bounds, drag.original, drag.rate, drag.edge, delta) {
                    drag.preview = preview;
                }
            } else {
                let cursor = drag.rate.snap_nearest((drag.original.start + delta).clamp(Tick::ZERO, drag.bounds.end()));
                drag.preview = TimeRange::from_bounds(drag.original.start.min(cursor), drag.original.start.max(cursor));
            }
        }
        let at = if drag.edge == RangeEdge::Out { drag.preview.end() - drag.rate.frame_duration() } else { drag.preview.start };
        if drag.preview.duration > Tick::ZERO {
            seek(app, item, at);
        }
        let preview = Rect::from_min_max(pos2(x(drag.preview.start).max(row.left()), row.top()), pos2(x(drag.preview.end()).min(row.right()), row.bottom()));
        if preview.is_positive() {
            ui.painter().rect_stroke(preview, 2.0, Stroke::new(2.0, Color32::LIGHT_GREEN), StrokeKind::Inside);
        }
        ui.painter().text(
            pos2(row.left() + 4.0, row.top() - 10.0),
            egui::Align2::LEFT_CENTER,
            format!("待剪 {:.3}–{:.3}s", drag.preview.start.seconds(), drag.preview.end().seconds()),
            Tokens::ui(10.0),
            Color32::LIGHT_BLUE,
        );
        if ctx.input(|i| i.pointer.any_released()) {
            app.selection_ranges.drag = None;
            commit(app, drag);
        } else if !ctx.input(|i| i.pointer.any_down()) {
            app.selection_ranges.drag = None;
        } else {
            app.selection_ranges.drag = Some(drag);
            ctx.request_repaint();
        }
    }
}
