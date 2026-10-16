//! Sound markers and coordinate hints are painted from the same derived engine mapping as automation.
use crate::panels::timeline::Layout;
use crate::{FilmcraftApp, theme::Tokens};
use egui::{Color32, Rect, Sense, pos2};
use filmcraft_project::{TrackId, TrackItem};
use filmcraft_time::Tick;

pub fn paint_clip(
    app: &mut FilmcraftApp,
    ui: &egui::Ui,
    p: &egui::Painter,
    track: TrackId,
    clip: &TrackItem,
    body: Rect,
    start: Tick,
    duration: Tick,
    layout: &Layout,
) {
    let displayed = if clip.start == start && clip.duration == duration {
        std::borrow::Cow::Borrowed(clip)
    } else {
        let mut preview = clip.clone();
        preview.start = start;
        preview.duration = duration;
        std::borrow::Cow::Owned(preview)
    };
    let source = format!(
        "成片 {:.3}–{:.3}s · 原片 {:.3}–{:.3}s",
        start.seconds(),
        (start + duration).seconds(),
        displayed.source_in.seconds(),
        displayed.source_out().seconds()
    );
    let visible = body.intersect(p.clip_rect());
    if !visible.is_positive() {
        return;
    }
    app.auto.add(&format!("timeline.clip.{}.coordinates", clip.id.0), visible, &source);
    ui.interact(visible, ui.id().with(("clip-coordinates", clip.id.0)), Sense::hover()).on_hover_text(&source);
    for event in filmcraft_engine::event_mapping::for_clip(&app.session, track, &displayed) {
        let (name, color, row) = if event.category == "gunshot" {
            ("枪声候选", Color32::from_rgb(238, 172, 79), 0)
        } else {
            ("人声候选", Color32::from_rgb(106, 191, 207), 1)
        };
        let top = (body.bottom() - 10.0 + row as f32 * 4.0).max(body.top() + 1.0);
        let left = layout.x_of(event.sequence_start);
        let right = layout.x_of(event.sequence_end).max(left + 1.0);
        let rect = Rect::from_min_max(pos2(left, top), pos2(right, top + 3.0)).intersect(visible);
        if !rect.is_positive() {
            continue;
        }
        p.rect_filled(rect, 1.0, color);
        let label = format!(
            "{name} · 成片 {:.3}–{:.3}s · 原片覆盖 {:.3}–{:.3}s\n原始事件 {:.3}–{:.3}s；仅显示片段覆盖部分",
            event.sequence_start.seconds(),
            event.sequence_end.seconds(),
            event.source_start.seconds(),
            event.source_end.seconds(),
            event.event_start.seconds(),
            event.event_end.seconds()
        );
        let id = format!("timeline.event.{}.{}.{}", clip.id.0, event.category, event.event_id);
        app.auto.add(&id, rect, &label);
        ui.interact(rect, ui.id().with(&id), Sense::hover()).on_hover_text(label);
    }
}

pub fn paint_coordinates(app: &mut FilmcraftApp, ui: &egui::Ui, rect: Rect, layout: &Layout) {
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p) && p.x >= layout.content.left());
    let time = pointer
        .map(|p| app.session.active_sequence().map_or(layout.tick_at(p.x), |q| q.settings.frame_rate.snap_nearest(layout.tick_at(p.x))).max(Tick::ZERO))
        .unwrap_or_else(|| app.session.playhead());
    let positions = filmcraft_engine::event_mapping::positions_at(&app.session, time);
    let row = pointer.and_then(|p| layout.row_at(p.y));
    let preferred = row.and_then(|row| positions.iter().find(|p| p.tracks.contains(&row.track))).or_else(|| positions.first());
    let label = preferred
        .map(|position| {
            format!(
                "成片 {:.3}s ↔ 原片 {:.3}s · {}",
                time.seconds(),
                position.source_time.seconds(),
                app.session.project.item(position.item).map_or("", |item| item.name.as_str())
            )
        })
        .unwrap_or_else(|| format!("成片 {:.3}s · 此处无原片", time.seconds()));
    let area = Rect::from_min_max(pos2(layout.content.left() + 6.0, rect.top() + 1.0), pos2(rect.right() - 6.0, rect.top() + 20.0));
    if area.is_positive() {
        ui.painter_at(area).text(area.left_center(), egui::Align2::LEFT_CENTER, &label, Tokens::ui(11.0), app.tokens.text_dim);
        app.auto.add("timeline.coordinates", area, &label);
        ui.interact(area, ui.id().with("sequence-source-coordinates"), Sense::hover())
            .on_hover_text("成片位置与其对应的原片位置。悬停片段查看该处坐标；时间尺定位节目预览，F 回原片。");
    }
}
pub fn source_coordinates(app: &FilmcraftApp, ui: &egui::Ui, item: filmcraft_project::ItemId, area: Rect, x0: f32) -> (Tick, String) {
    let source = app.session.project.item(item);
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| area.contains(*p) && p.x >= x0);
    let time =
        pointer.map(|p| Tick::from_seconds_f64(app.ui.timeline.scroll + (p.x - x0) as f64 / app.ui.timeline.pps)).unwrap_or(app.session.state.source_playhead);
    let time = source.map_or(time, |source| source.frame_rate().snap_nearest(time).clamp(Tick::ZERO, source.duration()));
    let positions = filmcraft_engine::event_mapping::positions_for_source(&app.session, item, time);
    let label = if positions.is_empty() {
        "未用于成片".into()
    } else {
        let times = positions
            .iter()
            .take(3)
            .map(|p| {
                p.sequence_end
                    .map(|end| format!("{:.3}–{:.3}s", p.sequence_time.seconds(), end.seconds()))
                    .unwrap_or_else(|| format!("{:.3}s", p.sequence_time.seconds()))
            })
            .collect::<Vec<_>>()
            .join(" / ");
        format!("对应成片 {times} · {}处", positions.len())
    };
    (time, label)
}
