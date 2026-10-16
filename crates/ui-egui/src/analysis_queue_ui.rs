//! Serial worker orchestration. Dialog selection never changes an in-flight job's origin.
use super::*;

pub(super) struct Active {
    pub(super) id: u64,
    pub(super) attempt: u64,
    pub(super) incarnation: u64,
    pub(super) validation: Option<(ItemId, String)>,
}
fn job(app: &FilmcraftApp, id: u64) -> Option<Value> {
    app.session.project.analysis_queue.iter().find(|t| t.id == id).and_then(|t| serde_json::to_value(t).ok())
}
fn selected(app: &FilmcraftApp) -> Option<Value> {
    job(app, app.apex_ai.selected_job?)
}
fn execute(app: &mut FilmcraftApp, id: &str, p: Value) -> Result<Value, String> {
    app.session.execute(id, p).map_err(|e| e.to_string())
}
pub(super) fn label(status: &str) -> &str {
    match status {
        "unanalyzed" => "未分析",
        "queued" => "排队中",
        "probing" => "读取音轨",
        "waiting_audio" => "等待试听与选轨",
        "running" => "分析中",
        "completed" => "已完成",
        "cancelled" => "已取消",
        "failed" => "失败",
        "unavailable" => "不可用",
        "interrupted" => "已中断，待主动继续",
        _ => "未知状态",
    }
}
pub(super) fn refresh(app: &mut FilmcraftApp) {
    association::refresh(app);
    app.apex_ai.queue = app.session.project.analysis_queue.iter().filter_map(|t| serde_json::to_value(t).ok()).collect();
    app.apex_ai.states = app
        .session
        .project
        .analysis_queue
        .iter()
        .map(|t| (t.item, format!("{}{}", label(&t.status), t.error.as_ref().map(|e| format!("：{e}")).unwrap_or_default())))
        .collect();
}
fn load(app: &mut FilmcraftApp, item: ItemId) {
    load_job(app, item, None);
}
fn load_job(app: &mut FilmcraftApp, item: ItemId, preferred: Option<u64>) {
    let task = preferred
        .and_then(|id| app.session.project.analysis_queue.iter().find(|t| t.id == id))
        .or_else(|| app.session.project.analysis_queue.iter().rev().find(|t| t.item == item))
        .cloned();
    app.apex_ai.selected_job = task.as_ref().map(|t| t.id);
    app.apex_ai.reference_item = Some(item);
    app.apex_ai.origin = Some(Origin { incarnation: app.session.recognition_jobs.incarnation, item });
    let record = app.session.project.recognition.get(&item).cloned();
    app.apex_ai.path = task
        .as_ref()
        .map(|t| t.source_path.clone())
        .or_else(|| {
            app.session.project.item(item).and_then(|i| i.as_media()).and_then(|m| match &m.media {
                MediaRef::File { path } => Some(path.clone()),
                _ => None,
            })
        })
        .unwrap_or_default();
    app.apex_ai.media = task.as_ref().and_then(|t| t.media.clone()).or_else(|| record.as_ref().map(|r| r["result"]["media"].clone()));
    app.apex_ai.audio_stream = task.as_ref().and_then(|t| t.audio_stream).or_else(|| record.as_ref().and_then(|r| r["audioStream"].as_u64()));
    app.apex_ai.pending = record.map(|r| r["result"].clone());
    app.apex_ai.stage =
        task.as_ref().map(|t| t.status.clone()).unwrap_or_else(|| if app.apex_ai.pending.is_some() { "completed" } else { "unanalyzed" }.into());
    app.apex_ai.message = label(&app.apex_ai.stage).into();
    app.apex_ai.error = task.and_then(|t| t.error);
    app.apex_ai.progress = None;
}
pub(super) fn open(app: &mut FilmcraftApp, p: &Value) -> Result<(), String> {
    let item = if let Some(value) = p.get("item") {
        ItemId(value.as_u64().ok_or("item 必须是素材编号")?)
    } else if let Some(value) = p.get("job") {
        let id = value.as_u64().ok_or("job 必须是任务编号")?;
        ItemId(job(app, id).and_then(|t| t["item"].as_u64()).ok_or("分析任务不存在")?)
    } else {
        app.session.state.source_item.ok_or("请先选择导入的原录像")?
    };
    if app.session.project.item(item).and_then(|i| i.as_media()).is_none() {
        return Err("原录像不存在".into());
    }
    app.clear_source_audition();
    app.apex_ai.auditioned.clear();
    app.apex_ai.audition_stream = None;
    load_job(app, item, p["job"].as_u64());
    app.apex_ai.open = true;
    Ok(())
}
fn ensure(app: &mut FilmcraftApp, p: &Value) -> Result<Value, String> {
    if let Some(value) = p.get("job") {
        let id = value.as_u64().ok_or("job 必须是任务编号")?;
        return job(app, id).ok_or("分析任务不存在".into());
    }
    let explicit_item = if let Some(value) = p.get("item") { Some(ItemId(value.as_u64().ok_or("item 必须是素材编号")?)) } else { None };
    let explicit_path = if let Some(value) = p.get("path") {
        Some(value.as_str().filter(|p| !p.is_empty() && p.len() <= 16_384 && !p.contains('\0')).ok_or("原片路径无效")?)
    } else {
        None
    };
    if explicit_item.is_none() && explicit_path.is_none() {
        if let Some(task) = selected(app) {
            return Ok(task);
        }
    }
    let item = if let Some(item) = explicit_item {
        item
    } else if let Some(path) = explicit_path {
        source_for_path(app, path).ok_or("请先导入该路径的录像")?
    } else {
        source_for_path(app, &app.apex_ai.path).or(app.session.state.source_item).ok_or("请先导入这份录像")?
    };
    if app.apex_ai.origin.as_ref().is_none_or(|o| o.item != item) {
        app.clear_source_audition();
        app.apex_ai.auditioned.clear();
        app.apex_ai.audition_stream = None;
    }
    let task = execute(app, "analysis.enqueue", json!({"item":item.0}))?;
    load_job(app, item, task["id"].as_u64());
    Ok(task)
}
pub(super) fn request(app: &mut FilmcraftApp, p: &Value, analyze: bool) -> Result<(), String> {
    let task = ensure(app, p)?;
    let status = task["status"].as_str().unwrap_or_default();
    if status == "waiting_audio" && analyze {
        let audio = match p.get("audioStream") {
            Some(v) => Some(v.as_u64().ok_or("audioStream 必须是非负整数")?),
            None => app.apex_ai.audio_stream,
        }
        .ok_or("请先选择并试听分析音轨")?;
        if !app.apex_ai.auditioned.contains(&audio) || app.source_playback.audition_failed() {
            return Err("请先单独试听所选音轨，等音频设备开始播放后确认分析".into());
        }
        execute(app, "analysis.selectAudio", json!({"job":task["id"],"attempt":task["attempt"],"audioStream":audio}))?;
        app.clear_source_audition();
        app.apex_ai.audition_stream = None;
    } else if matches!(status, "completed" | "cancelled" | "failed" | "unavailable" | "interrupted") {
        execute(app, "analysis.retry", json!({"job":task["id"]}))?;
        app.clear_source_audition();
        app.apex_ai.auditioned.clear();
        app.apex_ai.audition_stream = None;
    } else if status == "waiting_audio" && !analyze {
        execute(app, "analysis.cancel", json!({"job":task["id"]}))?;
        execute(app, "analysis.retry", json!({"job":task["id"]}))?;
    }
    if let Some(item) = task["item"].as_u64() {
        load_job(app, ItemId(item), task["id"].as_u64());
    }
    refresh(app);
    Ok(())
}
pub(super) fn cancel(app: &mut FilmcraftApp, p: &Value) -> Result<(), String> {
    let task = ensure(app, p)?;
    execute(app, "analysis.cancel", json!({"job":task["id"]}))?;
    #[cfg(not(target_arch = "wasm32"))]
    if app.apex_ai.active.as_ref().is_some_and(|a| Some(a.id) == task["id"].as_u64()) {
        if let Some(worker) = &mut app.apex_ai.worker {
            worker.cancel();
        }
    }
    if app.apex_ai.selected_job == task["id"].as_u64() {
        app.clear_source_audition();
        app.apex_ai.auditioned.clear();
        app.apex_ai.audition_stream = None;
        if let Some(item) = task["item"].as_u64() {
            load_job(app, ItemId(item), task["id"].as_u64());
        }
    }
    refresh(app);
    Ok(())
}
pub(super) fn retry(app: &mut FilmcraftApp, p: &Value) -> Result<(), String> {
    app.clear_source_audition();
    app.apex_ai.auditioned.clear();
    app.apex_ai.audition_stream = None;
    let task = ensure(app, p)?;
    execute(app, "analysis.retry", json!({"job":task["id"]}))?;
    if let Some(item) = task["item"].as_u64() {
        load_job(app, ItemId(item), task["id"].as_u64());
    }
    refresh(app);
    Ok(())
}
pub(super) fn can_confirm(app: &FilmcraftApp) -> bool {
    selected(app).is_none_or(|t| !matches!(t["status"].as_str(), Some("queued" | "probing" | "running")))
}

pub(super) fn poll(app: &mut FilmcraftApp, ctx: &egui::Context) {
    if app.apex_ai.incarnation != app.session.recognition_jobs.incarnation {
        app.clear_source_audition();
        app.apex_ai.auditioned.clear();
        app.apex_ai.audition_stream = None;
        app.apex_ai.media = None;
        app.apex_ai.audio_stream = None;
        app.apex_ai.origin = None;
        app.apex_ai.selected_job = None;
        app.apex_ai.pending = None;
        app.apex_ai.path.clear();
        app.apex_ai.stage = "unanalyzed".into();
        app.apex_ai.message = "未分析".into();
        app.apex_ai.error = None;
        app.apex_ai.progress = None;
        app.apex_ai.cached = false;
        app.apex_ai.states.clear();
        app.apex_ai.reference_item = app
            .session
            .project
            .assemblies
            .last()
            .map(|a| a.source_item)
            .filter(|id| app.session.project.item(*id).is_some())
            .or_else(|| app.session.project.recognition.keys().copied().find(|id| app.session.project.item(*id).is_some()))
            .or_else(|| app.session.project.selection_ranges.keys().copied().find(|id| app.session.project.item(*id).is_some()));
        if let Some(MediaRef::File { path }) =
            app.apex_ai.reference_item.and_then(|id| app.session.project.item(id)).and_then(|i| i.as_media()).map(|m| &m.media)
        {
            app.apex_ai.path = path.clone();
        }
        app.apex_ai.generated = app.session.project.assemblies.last().and_then(|a| serde_json::to_value(a).ok());
        app.apex_ai.job_progress.clear();
        association::restore(app);
        if let Some(item) = app.apex_ai.reference_item {
            load(app, item);
        }
        app.apex_ai.incarnation = app.session.recognition_jobs.incarnation;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(worker) = &mut app.apex_ai.worker {
            worker.cancel();
        }
    }
    if app.apex_ai.audition_stream.is_some() {
        if app.source_playback.audition_failed() {
            app.apex_ai.auditioned.clear();
            app.apex_ai.audition_stream = None;
            let message = format!("音轨试听失败：{}", app.ui.status);
            super::fail(app, message);
        } else if app.source_playback.clock.audio_clock && app.audio.as_ref().and_then(|a| a.played_frames()).is_some_and(|n| n > 0) {
            if let Some(audio) = app.apex_ai.audition_stream {
                app.apex_ai.auditioned.insert(audio);
            }
        }
    }
    // Register every import now, even while a previous request is running.
    while let Some(item) = app.session.recognition_jobs.imports.pop_front() {
        if app.session.project.item(item).and_then(|i| i.as_media()).is_none_or(|m| m.info.video.is_none()) {
            continue;
        }
        let _ = execute(app, "analysis.enqueue", json!({"item":item.0}));
        if app.apex_ai.reference_item.is_none() {
            load(app, item);
            if app.session.state.source_item.is_none() {
                let _ = app.session.execute("source.open", json!({"item":item.0}));
            }
        }
    }
    // A removed/offline item cannot remain a live request or block later work.
    let missing: Vec<_> = app
        .session
        .project
        .analysis_queue
        .iter()
        .filter(|t| {
            matches!(t.status.as_str(), "queued" | "waiting_audio" | "probing" | "running")
                && app.session.project.item(t.item).and_then(|i| i.as_media()).is_none_or(|m| m.offline)
        })
        .map(|t| (t.id, t.attempt))
        .collect();
    for (id, attempt) in missing {
        let _ = execute(app, "analysis.fail", json!({"job":id,"attempt":attempt,"code":"source_changed","error":"原片已移除或离线，旧任务已停止"}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some((id, attempt, incarnation)) = app.apex_ai.active.as_ref().map(|a| (a.id, a.attempt, a.incarnation)) {
            if incarnation == app.session.recognition_jobs.incarnation
                && job(app, id).is_some_and(|t| t["attempt"].as_u64() == Some(attempt) && matches!(t["status"].as_str(), Some("probing" | "running")))
            {
                if let Err(error) = execute(app, "analysis.checkOrigin", json!({"job":id})) {
                    let _ = execute(app, "analysis.fail", json!({"job":id,"attempt":attempt,"code":"source_changed","error":error}));
                }
            }
        }
        let current_active = app.apex_ai.active.as_ref().and_then(|a| job(app, a.id).map(|t| (a.incarnation, a.attempt, t)));
        if let Some(worker) = &mut app.apex_ai.worker {
            if app.apex_ai.active.as_ref().is_none_or(|a| a.validation.is_none() || a.incarnation != app.session.recognition_jobs.incarnation)
                && current_active.as_ref().is_none_or(|(inc, attempt, t)| {
                    *inc != app.session.recognition_jobs.incarnation
                        || t["attempt"].as_u64() != Some(*attempt)
                        || !matches!(t["status"].as_str(), Some("probing" | "running"))
                })
            {
                worker.cancel();
            }
            worker.tick_cancel();
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut events = Vec::new();
        if let Some(worker) = &app.apex_ai.worker {
            if let Some(rx) = &worker.receiver {
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
            }
        }
        for event in events {
            let Some(event) = association::consume(app, event) else { continue };
            let Some(active) = app.apex_ai.active.as_ref() else {
                continue;
            };
            let (id, attempt, incarnation) = (active.id, active.attempt, active.incarnation);
            match event {
                native::Event::Progress { stage, message, progress } => {
                    let valid =
                        job(app, id).is_some_and(|t| t["attempt"].as_u64() == Some(attempt) && matches!(t["status"].as_str(), Some("probing" | "running")));
                    if incarnation == app.session.recognition_jobs.incarnation && valid {
                        app.apex_ai.job_progress.insert(id, (message.clone(), progress));
                        if app.apex_ai.selected_job == Some(id) {
                            app.apex_ai.message = message;
                            app.apex_ai.stage = stage;
                            app.apex_ai.progress = progress;
                        }
                    }
                }
                native::Event::Finished(outcome) => {
                    let cancelled = app.apex_ai.worker.as_ref().is_some_and(|w| w.cancelled());
                    if let Some(mut worker) = app.apex_ai.worker.take() {
                        worker.finish();
                    }
                    app.apex_ai.active = None;
                    if incarnation != app.session.recognition_jobs.incarnation {
                        continue;
                    }
                    if cancelled {
                        if job(app, id).is_some_and(|t| t["attempt"].as_u64() == Some(attempt) && matches!(t["status"].as_str(), Some("probing" | "running"))) {
                            let _ = execute(app, "analysis.cancel", json!({"job":id}));
                        }
                    } else {
                        match outcome {
                            Ok(native::Outcome::Media(media)) => {
                                if let Ok(task) = execute(app, "analysis.probed", json!({"job":id,"attempt":attempt,"media":media})) {
                                    if task["status"] == "waiting_audio" {
                                        app.apex_ai.open = true;
                                    }
                                }
                            }
                            Ok(native::Outcome::Result { result, cached }) => {
                                let _ = execute(app, "analysis.complete", json!({"job":id,"attempt":attempt,"result":result}));
                                app.apex_ai.cached = cached;
                            }
                            Ok(native::Outcome::Validation(_)) => {
                                let _ = execute(app, "analysis.fail", json!({"job":id,"attempt":attempt,"error":"分析请求返回了错误的关联核对类型"}));
                            }
                            Err(failure) => {
                                let _ = execute(app, "analysis.fail", json!({"job":id,"attempt":attempt,"error":failure.message,"code":failure.code}));
                            }
                        }
                    }
                    if app.apex_ai.selected_job == Some(id) {
                        if let Some(task) = job(app, id) {
                            if let Some(item) = task["item"].as_u64() {
                                load_job(app, ItemId(item), task["id"].as_u64());
                            }
                        }
                    }
                }
            }
        }
        if !app.apex_ai.busy() {
            if let Ok(next) = execute(app, "analysis.next", json!({})) {
                if !next.is_null() {
                    if let Ok(task) = execute(app, "analysis.start", json!({"job":next["id"],"attempt":next["attempt"]})) {
                        if let (Some(id), Some(attempt), Some(path)) = (task["id"].as_u64(), task["attempt"].as_u64(), task["source_path"].as_str()) {
                            if matches!(task["status"].as_str(), Some("probing" | "running")) {
                                let audio = if task["status"] == "running" { task["audio_stream"].as_u64() } else { None };
                                match native::Worker::start(path, audio, ctx.clone()) {
                                    Ok(worker) => {
                                        app.apex_ai.active =
                                            Some(Active { id, attempt, incarnation: app.session.recognition_jobs.incarnation, validation: None });
                                        app.apex_ai.worker = Some(worker);
                                    }
                                    Err(message) => {
                                        let _ = execute(app, "analysis.fail", json!({"job":id,"attempt":attempt,"error":message}));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    association::poll(app, ctx);
    refresh(app);
}
