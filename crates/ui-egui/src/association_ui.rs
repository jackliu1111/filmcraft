//! Restored associations share the serial bridge worker, but never submit analysis.
use super::*;

pub(super) fn refresh(app: &mut FilmcraftApp) {
    let ids: std::collections::BTreeSet<_> = app
        .session
        .project
        .recognition
        .keys()
        .chain(app.session.project.selection_ranges.keys())
        .copied()
        .chain(app.session.project.assemblies.iter().map(|a| a.source_item))
        .chain(app.session.project.analysis_queue.iter().filter(|t| t.result_id.is_some()).map(|t| t.item))
        .collect();
    app.apex_ai.recovery = ids.into_iter().filter_map(|id| app.session.execute("recognition.recovery", json!({"item":id.0})).ok().map(|v| (id, v))).collect();
    if let Some(active) = &app.apex_ai.active {
        if active.incarnation == app.session.recognition_jobs.incarnation {
            if let Some((item, _)) = &active.validation {
                if let Some(state) = app.apex_ai.recovery.get_mut(item) {
                    state["status"] = json!("checking");
                    state["reason"] = json!("正在只读核对已存分析，不会重新推理；保存的人工工作仍保留");
                }
            }
        }
    }
}
pub(super) fn restore(app: &mut FilmcraftApp) {
    app.apex_ai.validation_pending = app.session.project.recognition.keys().copied().collect();
    app.apex_ai.recovery.clear();
}
pub(super) fn request(app: &mut FilmcraftApp, p: &Value) -> Result<(), String> {
    let id = if let Some(item) = p.get("item") {
        ItemId(item.as_u64().ok_or("item 必须是素材编号")?)
    } else {
        app.apex_ai.reference_item.or(app.session.state.source_item).ok_or("请先打开原片")?
    };
    if !app.session.project.recognition.contains_key(&id) {
        return Err("工程没有完整分析关联，请主动重试分析；已有人工范围仍保留".into());
    }
    if app
        .apex_ai
        .active
        .as_ref()
        .is_some_and(|a| a.incarnation == app.session.recognition_jobs.incarnation && a.validation.as_ref().is_some_and(|(item, _)| *item == id))
    {
        return Ok(());
    }
    app.apex_ai.validation_pending.retain(|i| *i != id);
    app.apex_ai.validation_pending.push_front(id);
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn poll(app: &mut FilmcraftApp, ctx: &egui::Context) {
    if app.apex_ai.busy() {
        return;
    }
    while let Some(item) = app.apex_ai.validation_pending.pop_front() {
        let Ok(state) = app.session.execute("recognition.recovery", json!({"item":item.0})) else { continue };
        if matches!(state["status"].as_str(), Some("source_offline" | "missing" | "unanalyzed")) {
            continue;
        }
        let (Some(path), Some(audio), Some(id)) = (state["path"].as_str(), state["audioStream"].as_u64(), state["resultId"].as_str()) else { continue };
        match native::Worker::start_validation(path, audio, id, ctx.clone()) {
            Ok(worker) => {
                app.apex_ai.active =
                    Some(queue::Active { id: 0, attempt: 0, incarnation: app.session.recognition_jobs.incarnation, validation: Some((item, id.to_string())) });
                app.apex_ai.worker = Some(worker);
            }
            Err(error) => {
                let _ = app.session.execute(
                    "recognition.validated",
                    json!({"item":item.0,"resultId":id,"projectToken":app.session.recognition_jobs.incarnation,"error":error,"code":"runtime_unavailable"}),
                );
            }
        }
        break;
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn consume(app: &mut FilmcraftApp, event: native::Event) -> Option<native::Event> {
    let Some((item, identifier)) = app.apex_ai.active.as_ref().and_then(|a| a.validation.clone()) else { return Some(event) };
    if let native::Event::Finished(outcome) = event {
        let incarnation = app.apex_ai.active.as_ref().map(|a| a.incarnation);
        let cancelled = app.apex_ai.worker.as_ref().is_some_and(|w| w.cancelled());
        if let Some(mut worker) = app.apex_ai.worker.take() {
            worker.finish();
        }
        app.apex_ai.active = None;
        if cancelled || incarnation != Some(app.session.recognition_jobs.incarnation) {
            return None;
        }
        let mut params = json!({"item":item.0,"resultId":identifier,"projectToken":incarnation});
        match outcome {
            Ok(native::Outcome::Validation(value)) => params["validation"] = value,
            Err(failure) => {
                params["error"] = json!(failure.message);
                params["code"] = json!(failure.code);
            }
            _ => {
                params["error"] = json!("只读核对未返回关联状态，未启动重新分析");
                params["code"] = json!("association_invalid");
            }
        }
        let _ = app.session.execute("recognition.validated", params);
    }
    None
}
