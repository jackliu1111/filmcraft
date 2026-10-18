//! AI/manual ranges enter the ordinary source-drag path; only context validation is added.
use crate::FilmcraftApp;
use crate::panels::{self, source_drag::SourceDrag, timeline::Layout};
use egui::{Color32, Rect, Stroke, StrokeKind, pos2};
use filmcraft_engine::{
    EngineError,
    selection_placement::{self, Snapshot},
};
use filmcraft_project::{ItemId, Sequence};
use serde_json::{Value, json};

#[derive(Clone)]
struct Flash {
    project: u64,
    sequence: ItemId,
    clips: Vec<u64>,
    until: f64,
}
fn flash_key() -> egui::Id {
    egui::Id::new("selection-placement-flash")
}
pub fn usage_label(app: &FilmcraftApp, item: ItemId, range: filmcraft_time::TimeRange) -> String {
    let usage = filmcraft_engine::selection_usage::usage(&app.session, item, range);
    match usage.status {
        "used" => format!("已使用 · {} 处（可重复）", usage.uses),
        "partial" => format!("部分重叠 · {:.2}s（可重复）", usage.covered.seconds()),
        _ => "未使用".into(),
    }
}
pub fn begin(app: &mut FilmcraftApp, ui: &egui::Ui, item: ItemId, range_id: &str) -> Result<(), String> {
    if panels::drag_cancelled(ui) {
        return Ok(());
    }
    let snapshot = selection_placement::capture(&app.session, item, range_id).map_err(|e| e.to_string())?;
    app.stop_source();
    app.ui.status = usage_label(app, item, snapshot.range());
    panels::start_drag_source(ui, SourceDrag { item, range: snapshot.range(), video: snapshot.video, audio: snapshot.audio, selection: Some(snapshot) });
    Ok(())
}
pub fn place(app: &mut FilmcraftApp, ctx: &egui::Context, snapshot: Snapshot, mut params: Value) -> Result<(), EngineError> {
    params["snapshot"] = json!(snapshot);
    let result = app.session.execute("selection.place", params)?;
    let clips = result["clips"].as_array().map(|ids| ids.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    let until = ctx.input(|i| i.time) + 0.65;
    ctx.data_mut(|data| data.insert_temp(flash_key(), Flash { project: snapshot.project_token, sequence: snapshot.sequence, clips, until }));
    app.ui.status = format!(
        "已加入原片 {:.3}–{:.3}s · {}",
        snapshot.source_in.seconds(),
        (snapshot.source_in + snapshot.duration).seconds(),
        usage_label(app, snapshot.item, snapshot.range())
    );
    ctx.request_repaint();
    Ok(())
}
pub fn append(app: &mut FilmcraftApp, ctx: &egui::Context, item: ItemId, range_id: &str) -> Result<(), String> {
    let snapshot = selection_placement::capture(&app.session, item, range_id).map_err(|e| e.to_string())?;
    place(app, ctx, snapshot, json!({"append":true})).map_err(|e| e.to_string())
}
pub fn paint_flash(app: &FilmcraftApp, ui: &egui::Ui, sequence: &Sequence, layout: &Layout) {
    let ctx = ui.ctx();
    let Some(flash) = ctx.data(|data| data.get_temp::<Flash>(flash_key())) else { return };
    let now = ctx.input(|i| i.time);
    if flash.until <= now || flash.project != app.session.recognition_jobs.incarnation || app.session.state.active_sequence != Some(flash.sequence) {
        ctx.data_mut(|data| data.remove::<Flash>(flash_key()));
        return;
    }
    let painter = ui.painter().with_clip_rect(layout.content);
    for clip in sequence.all_tracks().flat_map(|t| t.items.iter()).filter(|c| flash.clips.contains(&c.id.0)) {
        let Some((track, _)) = sequence.find_item(clip.id) else { continue };
        let Some(row) = layout.rows.iter().find(|r| r.track == track) else { continue };
        let rect = Rect::from_min_max(pos2(layout.x_of(clip.start), row.rect.top() + 1.0), pos2(layout.x_of(clip.end()), row.rect.bottom() - 1.0))
            .intersect(layout.content);
        if !rect.is_positive() {
            continue;
        }
        painter.rect_stroke(rect, 3.0, Stroke::new(2.5, Color32::LIGHT_BLUE.gamma_multiply(((flash.until - now) / 0.65) as f32)), StrokeKind::Inside);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
}
