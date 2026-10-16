//! Workspace-only source reference. These rows never enter the render sequence.
use crate::{FilmcraftApp, theme::Tokens};
use egui::{Align2, Color32, Rect, Sense, pos2, vec2};
use filmcraft_project::MediaRef;
use serde_json::json;

fn reference(app: &FilmcraftApp) -> Option<filmcraft_project::ItemId> {
    app.session
        .state
        .source_item
        .filter(|id| app.session.project.item(*id).and_then(|i| i.as_media()).is_some_and(|m| matches!(m.media, MediaRef::File { .. })))
        .or(app.apex_ai.reference_item)
        .or_else(|| app.session.project.recognition.keys().next().copied())
}
pub fn duration(app: &FilmcraftApp) -> f64 {
    reference(app).and_then(|id| app.session.project.item(id)).map_or(0.0, |i| i.duration().seconds())
}
pub fn reserve(app: &FilmcraftApp, rect: Rect) -> Rect {
    let height = if app.apex_ai.collapsed { 86.0 } else { 140.0 };
    if reference(app).is_some() && rect.height() >= height + 80.0 { Rect::from_min_max(pos2(rect.left(), rect.top() + height + 2.0), rect.max) } else { rect }
}

/// Reserve workspace rows above the native editing tracks, preserving the existing dock layout.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) -> Rect {
    crate::selection_ranges::begin_frame(app);
    let id = reference(app);
    let Some(id) = id else { return rect };
    let Some(item) = app.session.project.item(id) else { return rect };
    let name = item.name.clone();
    let duration = item.duration().seconds();
    let record = app.session.project.recognition.get(&id).cloned();
    let height = if app.apex_ai.collapsed { 86.0 } else { 140.0 };
    if rect.height() < height + 80.0 {
        return rect;
    }
    let area = Rect::from_min_size(rect.min, vec2(rect.width(), height));
    let head = app.ui.timeline.header_w;
    let x0 = area.left() + head;
    // Share the native timeline's wheel vocabulary, including when no edit sequence exists yet.
    if ui.rect_contains_pointer(area) {
        let wheel = crate::panels::timeline::wheel_input(ui.ctx());
        let pointer = ui.ctx().pointer_hover_pos().map_or(x0, |p| p.x);
        let v = &mut app.ui.timeline;
        if (wheel.pinch - 1.0).abs() > 1e-4 || (wheel.alt && wheel.delta.y != 0.0) {
            let factor = if (wheel.pinch - 1.0).abs() > 1e-4 { wheel.pinch as f64 } else { (1.0 + wheel.delta.y as f64 * 0.01).clamp(0.5, 2.0) };
            let anchor = v.scroll + (pointer - x0) as f64 / v.pps;
            v.pps = (v.pps * factor).clamp(0.05, 24_000.0);
            v.target_pps = v.pps;
            v.scroll = (anchor - (pointer - x0) as f64 / v.pps).max(0.0);
            v.target_scroll = v.scroll;
            v.fit_pending = false;
        } else {
            let sideways = (app.session.prefs.timeline.mouse_scrolling == "horizontal") != wheel.command;
            let delta = if wheel.delta.x != 0.0 {
                wheel.delta.x
            } else if sideways || wheel.shift {
                wheel.delta.y
            } else {
                0.0
            };
            if delta != 0.0 {
                v.scroll = (v.scroll - delta as f64 / v.pps).clamp(0.0, crate::panels::timeline::max_scroll(duration, area.width() - head, v.pps));
                v.target_scroll = v.scroll;
            }
        }
    }
    let pps = app.ui.timeline.pps;
    let scroll = app.ui.timeline.scroll;
    let x = |seconds: f64| x0 + ((seconds - scroll) * pps) as f32;
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, app.tokens.panel_bg);
    let (coordinate_time, corresponding) = crate::event_mapping::source_coordinates(app, ui, id, area, x0);
    p.text(
        pos2(area.left() + 6.0, area.top() + 10.0),
        Align2::LEFT_CENTER,
        format!("原片 {:.3}s · 参照", coordinate_time.seconds()),
        Tokens::ui(10.0),
        app.tokens.text_dim,
    );
    app.auto.add(
        "recognition.coordinates",
        Rect::from_min_max(area.min, pos2(area.right() - 80.0, area.top() + 20.0)),
        &format!("原片 {:.3}s · {corresponding}", coordinate_time.seconds()),
    );
    let status = record
        .as_ref()
        .map(|r| {
            let voice = &r["result"]["categories"]["voice"];
            let voice_status = if voice["status"] == "unavailable" || voice["events"].as_array().is_none() {
                "不可用".to_owned()
            } else {
                voice["events"].as_array().map_or(0, Vec::len).to_string()
            };
            format!("枪声 {} · 人声 {}", r["result"]["events"].as_array().map_or(0, Vec::len), voice_status)
        })
        .or_else(|| app.apex_ai.states.get(&id).cloned())
        .unwrap_or_else(|| "未分析".into());
    let label = Rect::from_min_max(pos2(x0, area.top()), pos2(area.right() - 80.0, area.top() + 20.0));
    ui.painter_at(label).text(label.left_center(), Align2::LEFT_CENTER, format!("{corresponding} · {name} · {status}"), Tokens::ui(11.0), app.tokens.text_dim);
    let analyze = Rect::from_min_size(pos2(area.right() - 74.0, area.top() + 1.0), vec2(68.0, 18.0));
    let r = ui.interact(analyze, ui.id().with("source-analysis"), Sense::click());
    app.auto.add("recognition.open", analyze, "声音识别");
    p.text(analyze.center(), Align2::CENTER_CENTER, "声音识别", Tokens::ui(11.0), app.tokens.accent);
    if r.clicked() {
        let _ = crate::apex_ai::invoke(app, ui.ctx(), "apexAi.open", &json!({"item":id.0}));
    }
    let step = if pps >= 60.0 {
        1.0
    } else if pps >= 12.0 {
        5.0
    } else if pps >= 3.0 {
        10.0
    } else {
        60.0
    };
    let mut time = (scroll / step).ceil() * step;
    for _ in 0..1000 {
        let at = x(time);
        if at > area.right() {
            break;
        }
        p.line_segment([pos2(at, area.top() + 33.0), pos2(at, area.top() + 38.0)], (1.0, app.tokens.text_dim));
        p.text(pos2(at + 3.0, area.top() + 29.0), Align2::LEFT_CENTER, format!("{time:.0}s"), Tokens::ui(10.0), app.tokens.text_dim);
        time += step;
    }
    for (row, text) in [(0, "V1 原片参照"), (1, if app.apex_ai.collapsed { "▸ A1 原片音频" } else { "▾ A1 原片音频" })] {
        let y = area.top() + 39.0 + row as f32 * 23.0;
        p.text(pos2(area.left() + 6.0, y + 10.0), Align2::LEFT_CENTER, text, Tokens::ui(11.0), app.tokens.text_dim);
        let bar = Rect::from_min_max(pos2(x(0.0).max(x0), y + 2.0), pos2(x(duration).min(area.right()), y + 20.0));
        if bar.is_positive() {
            p.rect_filled(bar, 2.0, Color32::from_rgb(43, 72, 93));
        }
    }
    let fold = Rect::from_min_size(pos2(area.left() + 5.0, area.top() + 62.0), vec2(head - 10.0, 21.0));
    let r = ui.interact(fold, ui.id().with("recognition-fold"), Sense::click());
    app.auto.add("recognition.audio.toggle", fold, "展开或折叠声音识别子行");
    if r.clicked() {
        app.apex_ai.collapsed = !app.apex_ai.collapsed;
    }
    if !app.apex_ai.collapsed {
        for (row, category, name, color) in
            [(0, "gunshot", "枪声候选", Color32::from_rgb(238, 172, 79)), (1, "voice", "人声候选", Color32::from_rgb(106, 191, 207))]
        {
            let y = area.top() + 86.0 + row as f32 * 26.0;
            let collapsed = if category == "gunshot" { app.apex_ai.gunshot_collapsed } else { app.apex_ai.voice_collapsed };
            let label = Rect::from_min_max(pos2(area.left() + 5.0, y), pos2(x0 - 5.0, y + 23.0));
            p.text(label.left_center(), Align2::LEFT_CENTER, format!("{} {name}", if collapsed { "▸" } else { "▾" }), Tokens::ui(11.0), color);
            let response = ui.interact(label, ui.id().with(("recognition-category-fold", category)), Sense::click());
            app.auto.add(&format!("recognition.{category}.toggle"), label, &format!("展开或折叠{name}子行"));
            if response.clicked() {
                if category == "gunshot" {
                    app.apex_ai.gunshot_collapsed = !collapsed;
                } else {
                    app.apex_ai.voice_collapsed = !collapsed;
                }
            }
            if collapsed {
                continue;
            }
            let category_record = record.as_ref().map(|r| &r["result"]["categories"][category]);
            let unavailable = category_record.is_none_or(|v| v.is_null() || v["status"] == "unavailable");
            let events = category_record
                .and_then(|v| v["events"].as_array())
                .or_else(|| if category == "gunshot" { record.as_ref().and_then(|r| r["result"]["events"].as_array()) } else { None });
            let message = if category == "voice" && unavailable {
                "暂无可用人声分数（需重新读取完整预测）"
            } else if events.is_some_and(Vec::is_empty) {
                "分析完成 · 无候选事件"
            } else if events.is_none() {
                "未分析"
            } else {
                ""
            };
            p.text(pos2(x0 + 5.0, y + 11.0), Align2::LEFT_CENTER, message, Tokens::ui(10.0), app.tokens.text_dim);
            if let Some(events) = events {
                for (index, event) in events.iter().enumerate() {
                    let (Some(start), Some(end)) = (event["start_s"].as_f64(), event["end_s"].as_f64()) else { continue };
                    let bar = Rect::from_min_max(pos2(x(start).max(x0), y + 2.0), pos2(x(end).min(area.right()), y + 21.0));
                    if !bar.is_positive() {
                        continue;
                    }
                    p.rect_filled(bar, 2.0, color.gamma_multiply(0.7));
                    let response = ui.interact(bar, ui.id().with(("recognition-event", id.0, category, index)), Sense::click());
                    app.auto.add(&format!("recognition.{category}.event.{index}"), bar, &format!("{name} 原片 {start:.2}–{end:.2} 秒"));
                    if response.clicked() {
                        let _ = app.session.execute("source.open", json!({"item":id.0}));
                        let _ = app.session.execute("source.setPlayhead", json!({"seconds":start}));
                    }
                    response.on_hover_text(format!("{name} · 原片 {start:.2}–{end:.2} 秒\n所选分析音轨的声音候选，不表示说话人身份或转写\n单击定位原片预览"));
                }
            }
            crate::selection_ranges::show(app, ui, id, category, Rect::from_min_max(pos2(x0, y), pos2(area.right(), y + 24.0)), x0, pps, scroll);
        }
    }
    let play = app.session.state.source_playhead.seconds();
    let play_x = x(play);
    if play_x >= x0 && play_x <= area.right() {
        p.line_segment([pos2(play_x, area.top() + 20.0), pos2(play_x, area.bottom())], (1.0, Color32::WHITE));
    }
    Rect::from_min_max(pos2(rect.left(), area.bottom() + 2.0), rect.max)
}
