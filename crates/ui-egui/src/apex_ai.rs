//! Native analysis adapter. Completion only attaches evidence; assembly requires an explicit action.

use std::sync::Arc;

use filmcraft_engine::{EngineError, Session};
use filmcraft_project::{ItemId, MediaRef};
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};

use crate::FilmcraftApp;

const MAX_INTERVALS: usize = 4096;
const MAX_EVENTS: usize = 20_000;

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub reference_item: Option<ItemId>,
    pub collapsed: bool,
    pub gunshot_collapsed: bool,
    pub voice_collapsed: bool,
    pub states: std::collections::BTreeMap<ItemId, String>,
    incarnation: u64,
    auto_analyze: bool,
    token: Option<Value>,
    path: String,
    media: Option<Value>,
    audio_stream: Option<u64>,
    auditioned: std::collections::BTreeSet<u64>,
    audition_stream: Option<u64>,
    stage: String,
    message: String,
    progress: Option<f32>,
    error: Option<String>,
    pending: Option<Value>,
    cached: bool,
    origin: Option<Origin>,
    generated: Option<Value>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<native::Worker>,
}

struct Origin {
    incarnation: u64,
    item: ItemId,
}
impl Origin {
    fn matches(&self, s: &Session) -> bool {
        self.incarnation == s.recognition_jobs.incarnation && s.project.item(self.item).is_some()
    }
}

impl State {
    fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.worker.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "open": self.open, "busy": self.busy(), "path": self.path,
            "sourceItem":self.reference_item.map(|i|i.0),"sources":self.states.iter().map(|(id,status)|json!({"item":id.0,"status":status})).collect::<Vec<_>>(),
            "audioStream": self.audio_stream, "auditionedStreams":self.auditioned, "media": self.media,
            "stage": self.stage, "message": self.message, "progress": self.progress,
            "error": self.error, "cached": self.cached, "pendingResult": self.pending.is_some(),
            "resultId": self.pending.as_ref().and_then(|r| r.get("result_id")),
            "generated": self.generated,
        })
    }

    pub fn shutdown(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mut worker) = self.worker.take() {
            worker.shutdown();
        }
    }
}

fn selected_path(app: &FilmcraftApp) -> Option<String> {
    app.session.state.project_selection.iter().copied().chain(app.session.state.source_item).find_map(|id| {
        let media = app.session.project.item(id)?.as_media()?;
        match &media.media {
            MediaRef::File { path } => Some(path.clone()),
            _ => None,
        }
    })
}

fn tracks(media: &Value) -> &[Value] {
    media.get("audio_tracks").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn source_for_path(app: &FilmcraftApp, path: &str) -> Option<ItemId> {
    app.apex_ai.origin.as_ref().filter(|o| o.matches(&app.session)).map(|o| o.item).into_iter().chain(app.session.project.items.keys().copied()).find(|id| {
        app.session
            .project
            .item(*id)
            .and_then(|i| i.as_media())
            .is_some_and(|m| matches!(&m.media,MediaRef::File {path:p} if std::fs::canonicalize(p).ok() == std::fs::canonicalize(path).ok()))
    })
}

/// UI/control commands share this entry point. No command starts a new or replacement project.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, id: &str, params: &Value) -> Result<Value, String> {
    match id {
        "apexAi.open" => {
            app.apex_ai.open = true;
            if !app.apex_ai.busy() {
                app.clear_source_audition();
                app.apex_ai.auditioned.clear();
                app.apex_ai.audition_stream = None;
                let item = params["item"].as_u64().map(ItemId).or(app.session.state.source_item);
                if let Some(item) = item {
                    if let Some(record) = app.session.project.recognition.get(&item).cloned() {
                        app.apex_ai.path = record["sourcePath"].as_str().unwrap_or_default().into();
                        app.apex_ai.pending = Some(record["result"].clone());
                        app.apex_ai.media = Some(record["result"]["media"].clone());
                        app.apex_ai.audio_stream = record["audioStream"].as_u64();
                        app.apex_ai.origin = Some(Origin { incarnation: app.session.recognition_jobs.incarnation, item });
                    } else if let Some(MediaRef::File { path }) = app.session.project.item(item).and_then(|i| i.as_media()).map(|m| &m.media) {
                        if app.apex_ai.path != *path {
                            app.apex_ai.media = None;
                            app.apex_ai.audio_stream = None;
                            app.apex_ai.pending = None;
                            app.apex_ai.origin = None;
                        }
                        app.apex_ai.path = path.clone();
                    }
                } else if app.apex_ai.path.is_empty() {
                    app.apex_ai.path = selected_path(app).unwrap_or_default();
                }
            }
        }
        "apexAi.close" => {
            app.apex_ai.open = false;
            app.clear_source_audition();
        }
        "apexAi.audition" => {
            if app.apex_ai.busy() {
                return Err("请等待当前分析任务停止后再试听".into());
            }
            let audio = match params.get("audioStream") {
                Some(value) => Some(value.as_u64().ok_or("audioStream 必须是非负整数")?),
                None => app.apex_ai.audio_stream,
            }
            .ok_or("请先选择试听音轨")?;
            let media = app.apex_ai.media.clone().ok_or("请先读取音轨")?;
            let source = source_for_path(app, &app.apex_ai.path).ok_or("请先导入这份录像")?;
            let mapping = app.session.execute("recognition.mapAudio", json!({"item":source.0,"media":media})).map_err(|e| e.to_string())?;
            let ordinal = mapping["audioMapping"]
                .as_array()
                .and_then(|rows| rows.iter().find(|r| r["containerStream"].as_u64() == Some(audio)))
                .and_then(|row| row["nativeOrdinal"].as_u64())
                .ok_or("音轨映射不存在")?;
            app.apex_ai.auditioned.remove(&audio);
            app.audition_source_audio(source, ordinal as usize)?;
            app.apex_ai.audio_stream = Some(audio);
            app.apex_ai.audition_stream = Some(audio);
            app.apex_ai.stage = "auditioning".into();
            app.apex_ai.error = None;
            app.apex_ai.message = format!("正在单独试听音轨 #{audio}；分析选择不改变成片保留的声音。");
        }
        "apexAi.stopAudition" => {
            app.clear_source_audition();
            app.apex_ai.audition_stream = None;
            if app.apex_ai.stage == "auditioning" {
                app.apex_ai.stage = "ready".into();
                app.apex_ai.message = "试听已停止，可确认所选音轨进行分析。".into();
            }
        }
        "apexAi.probe" | "apexAi.analyze" => {
            if app.apex_ai.busy() {
                return Err("AI 任务正在运行，请先等待或取消。".into());
            }
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| (!app.apex_ai.path.trim().is_empty()).then(|| app.apex_ai.path.clone()))
                .or_else(|| selected_path(app))
                .ok_or("请先选择原录像。")?;
            if path.len() > 16_384 || path.contains('\0') {
                return Err("录像路径无效。".into());
            }
            if id == "apexAi.analyze" {
                let media = app.apex_ai.media.as_ref().ok_or("请先读取录像和音轨。")?;
                let media_path = media.pointer("/source/path").and_then(Value::as_str).ok_or("媒体记录缺少原片路径。")?;
                if media_path != path {
                    return Err("录像已变化，请重新读取音轨。".into());
                }
                let audio = match params.get("audioStream") {
                    Some(v) => Some(v.as_u64().ok_or("audioStream 必须是非负整数。")?),
                    None => app.apex_ai.audio_stream,
                }
                .ok_or("请明确选择用于分析的游戏音轨。")?;
                if !tracks(media).iter().any(|t| t.get("stream_index").and_then(Value::as_u64) == Some(audio)) {
                    return Err("所选音轨不存在，请重新读取录像。".into());
                }
                app.apex_ai.audio_stream = Some(audio);
                if tracks(media).len() > 1 && (!app.apex_ai.auditioned.contains(&audio) || app.source_playback.audition_failed()) {
                    return Err("多音轨录像请先单独试听所选音轨，再确认分析。".into());
                }
                let source_item = source_for_path(app, &path).ok_or("请先导入这份录像。")?;
                app.apex_ai.origin = Some(Origin { incarnation: app.session.recognition_jobs.incarnation, item: source_item });
                app.apex_ai.token = Some(
                    app.session.execute("recognition.begin", json!({"item":source_item.0,"audioStream":audio,"media":media})).map_err(|e| e.to_string())?["token"].clone(),
                );
            } else {
                app.clear_source_audition();
                app.apex_ai.auditioned.clear();
                app.apex_ai.audition_stream = None;
                app.apex_ai.media = None;
                app.apex_ai.audio_stream = None;
            }
            if id == "apexAi.analyze" {
                app.clear_source_audition();
            }
            start(app, ctx, &path, id == "apexAi.analyze")?;
        }
        "apexAi.cancel" => {
            app.clear_source_audition();
            app.apex_ai.auditioned.clear();
            app.apex_ai.audition_stream = None;
            if let Some(token) = app.apex_ai.token.take() {
                let _ = app.session.execute("recognition.cancel", json!({"token":token}));
            }
            app.apex_ai.auto_analyze = false;
            if !app.apex_ai.busy() {
                app.apex_ai.audio_stream = None;
                app.apex_ai.stage = "cancelled".into();
                app.apex_ai.message = "已取消音轨选择，已有工程未改变。".into();
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(w) = &mut app.apex_ai.worker {
                w.cancel();
                app.apex_ai.stage = "cancelling".into();
                app.apex_ai.message = "正在停止 AI 任务并清理进程…".into();
            }
        }
        "apexAi.apply" => {
            if app.apex_ai.busy() {
                return Err("请等待当前任务停止。".into());
            }
            let result = app.apex_ai.pending.clone().ok_or("没有已完成的 AI 结果。")?;
            if !app.apex_ai.origin.as_ref().is_some_and(|o| o.matches(&app.session)) {
                return Err("项目或源监视器已切换。结果已保留；确认后可加入当前项目。".into());
            }
            apply_result(app, &result)?;
        }
        "apexAi.state" | "apexAi.inspect" => {}
        _ => return Err("未知 AI 剪辑命令。".into()),
    }
    ctx.request_repaint();
    Ok(app.apex_ai.snapshot())
}

fn start(app: &mut FilmcraftApp, ctx: &egui::Context, path: &str, analyze: bool) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = std::fs::canonicalize(path).map_err(|e| format!("无法读取录像：{e}"))?;
        if !path.is_file() {
            return Err("请选择本地录像文件。".into());
        }
        let path = path.to_str().ok_or("录像路径不是有效文本。")?.to_string();
        let worker = native::Worker::start(&path, analyze.then_some(app.apex_ai.audio_stream).flatten(), ctx.clone())?;
        app.apex_ai.worker = Some(worker);
        app.apex_ai.path = path;
        app.apex_ai.stage = if analyze { "analyzing" } else { "probing" }.into();
        app.apex_ai.message = if analyze { "正在使用已有模型分析录像…" } else { "正在读取录像与音轨…" }.into();
        app.apex_ai.error = None;
        app.apex_ai.progress = None;
        app.apex_ai.generated = None;
        app.apex_ai.pending = None;
        app.apex_ai.cached = false;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, ctx, path, analyze);
        Err("本地 AI 原型只支持桌面版。".into())
    }
}

/// Poll before normal editor events so opening the new sequence reaches the native timeline.
pub fn poll(app: &mut FilmcraftApp, ctx: &egui::Context) {
    if app.apex_ai.incarnation != app.session.recognition_jobs.incarnation {
        app.clear_source_audition();
        app.apex_ai.auditioned.clear();
        app.apex_ai.audition_stream = None;
        app.apex_ai.audio_stream = None;
        app.apex_ai.media = None;
        app.apex_ai.origin = None;
        app.apex_ai.incarnation = app.session.recognition_jobs.incarnation;
        app.apex_ai.states.clear();
        app.apex_ai.reference_item = None;
        app.apex_ai.pending = None;
        app.apex_ai.token = None;
        app.apex_ai.auto_analyze = false;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(w) = &mut app.apex_ai.worker {
            w.cancel();
        }
    }
    if app.apex_ai.stage == "auditioning" {
        if app.source_playback.audition_failed() {
            app.apex_ai.auditioned.clear();
            app.apex_ai.audition_stream = None;
            let error = format!("音轨试听失败：{}", app.ui.status);
            fail(app, error);
        } else if app.source_playback.clock.audio_clock && app.audio.as_ref().and_then(|a| a.played_frames()).is_some_and(|n| n > 0) {
            if let Some(audio) = app.apex_ai.audition_stream {
                app.apex_ai.auditioned.insert(audio);
            }
        }
    }
    if !app.apex_ai.busy() {
        while let Some(item) = app.session.recognition_jobs.imports.pop_front() {
            let Some(media) = app.session.project.item(item).and_then(|i| i.as_media()) else { continue };
            let MediaRef::File { path } = &media.media else { continue };
            let path = path.clone();
            let has_audio = !media.info.audio_streams.is_empty();
            if media.info.video.is_none() {
                continue;
            }
            app.apex_ai.reference_item = Some(item);
            if app.session.state.source_item.is_none() {
                let _ = app.session.execute("source.open", json!({"item":item.0}));
            }
            if !has_audio {
                app.apex_ai.states.insert(item, "不可分析：原片无音轨".into());
                continue;
            }
            app.apex_ai.auto_analyze = true;
            app.apex_ai.origin = Some(Origin { incarnation: app.session.recognition_jobs.incarnation, item });
            app.apex_ai.states.insert(item, "读取音轨…".into());
            app.clear_source_audition();
            app.apex_ai.auditioned.clear();
            app.apex_ai.audition_stream = None;
            app.apex_ai.media = None;
            app.apex_ai.audio_stream = None;
            if let Err(e) = start(app, ctx, &path, false) {
                fail(app, e);
            }
            break;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut events = Vec::new();
        if let Some(w) = &mut app.apex_ai.worker {
            w.tick_cancel();
            if let Some(rx) = &w.receiver {
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        for event in events {
            match event {
                native::Event::Progress { stage, message, progress } => {
                    app.apex_ai.stage = stage;
                    app.apex_ai.message = message;
                    app.apex_ai.progress = progress;
                    if let Some(origin) = &app.apex_ai.origin {
                        if origin.matches(&app.session) {
                            app.apex_ai
                                .states
                                .insert(origin.item, format!("{} {}", app.apex_ai.message, progress.map(|p| format!("{:.0}%", p * 100.0)).unwrap_or_default()));
                        }
                    }
                }
                native::Event::Finished(outcome) => {
                    let cancelled = app.apex_ai.worker.as_ref().is_some_and(|w| w.cancelled());
                    if let Some(mut w) = app.apex_ai.worker.take() {
                        w.finish();
                    }
                    if cancelled {
                        app.apex_ai.stage = "cancelled".into();
                        app.apex_ai.message = "AI 任务已取消，已有工程未改变。".into();
                        app.apex_ai.progress = None;
                        if let Some(origin) = &app.apex_ai.origin {
                            if origin.matches(&app.session) {
                                app.apex_ai.states.insert(origin.item, "分析已取消".into());
                            }
                        }
                        continue;
                    }
                    match outcome {
                        Ok(native::Outcome::Media(media)) => {
                            let list = tracks(&media);
                            if list.is_empty() {
                                app.apex_ai.media = Some(media);
                                app.apex_ai.audio_stream = None;
                                app.apex_ai.auto_analyze = false;
                                app.apex_ai.stage = "unavailable".into();
                                app.apex_ai.message = "原片没有可供分析的音轨；已有工程未改变。".into();
                                if let Some(origin) = app.apex_ai.origin.as_ref().filter(|o| o.matches(&app.session)) {
                                    app.apex_ai.states.insert(origin.item, "不可分析：原片没有可用音轨".into());
                                }
                                continue;
                            }
                            app.apex_ai.audio_stream =
                                if list.len() == 1 { list.first().and_then(|t| t.get("stream_index")).and_then(Value::as_u64) } else { None };
                            app.apex_ai.media = Some(media);
                            app.apex_ai.stage = "ready".into();
                            app.apex_ai.message = if app.apex_ai.audio_stream.is_some() {
                                "音轨已就绪。"
                            } else {
                                "多音轨录像：请先单独试听，再选择音轨并确认分析。"
                            }
                            .into();
                            if app.apex_ai.auto_analyze && app.apex_ai.audio_stream.is_some() {
                                app.apex_ai.auto_analyze = false;
                                if let Err(e) = invoke(app, ctx, "apexAi.analyze", &json!({})) {
                                    fail(app, e);
                                }
                            } else if app.apex_ai.auto_analyze {
                                app.apex_ai.auto_analyze = false;
                                app.apex_ai.open = true;
                            }
                        }
                        Ok(native::Outcome::Result { result, cached }) => {
                            app.apex_ai.cached = cached;
                            app.apex_ai.pending = Some(result.clone());
                            app.apex_ai.progress = Some(1.0);
                            if app.apex_ai.origin.as_ref().is_some_and(|o| o.matches(&app.session)) {
                                let token = app.apex_ai.token.take().unwrap_or(Value::Null);
                                match app.session.execute("recognition.attach", json!({"token":token,"result":result})) {
                                    Ok(_) => {
                                        app.apex_ai.stage = "completed".into();
                                        app.apex_ai.message = format!("枪声识别完成：{} 个候选事件。", result["events"].as_array().map_or(0, Vec::len));
                                        if let Some(origin) = &app.apex_ai.origin {
                                            app.apex_ai.states.remove(&origin.item);
                                        }
                                    }
                                    Err(e) => fail(app, e.to_string()),
                                }
                            } else {
                                app.apex_ai.pending = None;
                                app.apex_ai.stage = "stale".into();
                                app.apex_ai.message = "原工程已切换，迟到结果没有写入当前工程。".into();
                            }
                        }
                        Err(e) => fail(app, e),
                    }
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}

fn fail(app: &mut FilmcraftApp, message: String) {
    if let Some(token) = app.apex_ai.token.take() {
        let _ = app.session.execute("recognition.cancel", json!({"token":token}));
    }
    app.apex_ai.pending = None;
    app.apex_ai.auto_analyze = false;
    if let Some(origin) = &app.apex_ai.origin {
        if origin.matches(&app.session) {
            app.apex_ai.states.insert(origin.item, format!("分析失败：{message}"));
        }
    }
    app.ui.status = message.clone();
    app.apex_ai.stage = "failed".into();
    app.apex_ai.error = Some(message.clone());
    app.apex_ai.message = message;
}

pub fn show(app: &mut FilmcraftApp, ctx: &egui::Context) {
    if !app.apex_ai.open {
        return;
    }
    let mut open = true;
    let mut action: Option<(&str, Value)> = None;
    egui::Window::new("AI 剪辑").id(egui::Id::new("apex-ai-window")).open(&mut open).default_width(560.0).show(ctx, |ui| {
        ui.label("导入录像 → 音轨试听与声音识别 → 原片事件浏览 → 手动选片");
        ui.small("枪声候选不代表本人开枪、命中或击杀。导出沿用编辑器原生设置。");
        let busy = app.apex_ai.busy();
        ui.add_enabled_ui(!busy, |ui| {
            let path = ui.text_edit_singleline(&mut app.apex_ai.path);
            app.auto.add("apexAi.path", path.rect, "原录像路径");
            ui.horizontal(|ui| {
                let current = ui.button("使用当前素材");
                app.auto.add("apexAi.current", current.rect, "使用当前素材");
                if current.clicked() {
                    if let Some(path) = selected_path(app) {
                        action = Some(("apexAi.probe", json!({"path":path})));
                    } else {
                        app.apex_ai.error = Some("请在素材区选中一份本地录像。".into());
                    }
                }
                let choose = ui.button("选择录像…");
                app.auto.add("apexAi.choose", choose.rect, "选择录像");
                if choose.clicked()
                    && let Some(path) = app.hooks.pick_open_file.as_mut().and_then(|pick| pick("录像", &["mp4", "mkv", "mov", "avi", "webm"]))
                {
                    action = Some(("apexAi.probe", json!({"path":path})));
                }
                let probe = ui.button("读取音轨");
                app.auto.add("apexAi.probe", probe.rect, "读取音轨");
                if probe.clicked() {
                    action = Some(("apexAi.probe", json!({})));
                }
            });
            if let Some(media) = app.apex_ai.media.clone() {
                for track in tracks(&media) {
                    let Some(index) = track.get("stream_index").and_then(Value::as_u64) else { continue };
                    let label = format!(
                        "音轨 #{index} · {} · {} 声道 · {} Hz",
                        track.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()).unwrap_or("未命名"),
                        track.get("channels").and_then(Value::as_u64).unwrap_or(0),
                        track.get("sample_rate").and_then(Value::as_u64).unwrap_or(0)
                    );
                    ui.horizontal(|ui| {
                        let radio = ui.radio_value(&mut app.apex_ai.audio_stream, Some(index), label.clone());
                        app.auto.add(&format!("apexAi.audio.{index}"), radio.rect, &label);
                        let audition = ui.button("单独试听");
                        app.auto.add(&format!("apexAi.audition.{index}"), audition.rect, &format!("单独试听音轨 #{index}"));
                        if audition.clicked() {
                            action = Some(("apexAi.audition", json!({"audioStream":index})));
                        }
                    });
                }
                ui.small("分析音轨仅供识别；加入剪辑轨时默认保留原片全部可用声音。");
                let stop = ui.button("停止试听");
                app.auto.add("apexAi.stopAudition", stop.rect, "停止试听");
                if stop.clicked() {
                    action = Some(("apexAi.stopAudition", json!({})));
                }
            }
            let ready = app.apex_ai.audio_stream.is_some_and(|audio| {
                app.apex_ai.media.as_ref().is_some_and(|m| tracks(m).len() == 1)
                    || (app.apex_ai.auditioned.contains(&audio) && !app.source_playback.audition_failed())
            });
            let analyze = ui.add_enabled(ready, egui::Button::new("确认音轨并分析声音"));
            app.auto.add("apexAi.analyze", analyze.rect, "确认音轨并分析声音");
            if analyze.clicked() {
                action = Some(("apexAi.analyze", json!({})));
            }
        });
        if busy {
            match app.apex_ai.progress {
                Some(p) => {
                    ui.add(egui::ProgressBar::new(p).show_percentage());
                }
                None => {
                    ui.spinner();
                }
            }
            let cancel = ui.button("取消 AI 任务");
            app.auto.add("apexAi.cancel", cancel.rect, "取消 AI 任务");
            if cancel.clicked() {
                action = Some(("apexAi.cancel", json!({})));
            }
        }
        ui.label(&app.apex_ai.message);
        if let Some(error) = &app.apex_ai.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if app.apex_ai.pending.is_some() && app.apex_ai.generated.is_none() && !busy {
            let apply = ui.button("按当前范围组稿");
            app.auto.add("apexAi.apply", apply.rect, "按当前范围组稿");
            if apply.clicked() {
                action = Some(("apexAi.apply", json!({"confirmCurrentProject":true})));
            }
        }
        if let Some(generated) = &app.apex_ai.generated {
            ui.separator();
            ui.label(if app.apex_ai.cached { "复用了已有分析结果" } else { "已完成模型分析" });
            if let Some(path) = generated.get("originalProject").and_then(Value::as_str) {
                ui.small(format!("AI 原稿：{path}"));
            }
            if let Some(path) = generated.get("editProject").and_then(Value::as_str) {
                ui.small(format!("当前编辑副本：{path}"));
            }
            ui.small("可在时间线删段、改边界、补入原片；完成后使用原生导出。");
        }
    });
    app.apex_ai.open = open;
    if !open {
        app.clear_source_audition();
        app.apex_ai.auditioned.clear();
        app.apex_ai.audition_stream = None;
    }
    if let Some((command, params)) = action
        && let Err(e) = invoke(app, ctx, command, &params)
    {
        fail(app, e);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Interval {
    start: Tick,
    end: Tick,
}

fn tick(value: &Value) -> Result<Tick, String> {
    let seconds = value.as_f64().filter(|v| v.is_finite() && *v >= 0.0 && *v <= Tick::MAX.seconds()).ok_or("AI 时间范围无效。")?;
    Ok(Tick::from_seconds_f64(seconds))
}

fn intervals(result: &Value, rate: FrameRate, duration: Tick) -> Result<Vec<Interval>, String> {
    let items = result.get("selection").or_else(|| result.get("candidates")).and_then(Value::as_array).ok_or("AI 结果缺少候选范围。")?;
    if items.len() > MAX_INTERVALS {
        return Err("候选数量超出本原型支持范围。".into());
    }
    let limit = rate.snap(duration);
    let mut ranges = Vec::new();
    for item in items {
        if item.get("selected").is_some_and(|value| !value.is_boolean()) {
            return Err("候选选择状态无效。".into());
        }
        if item.get("selected").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let start = tick(item.get("start_s").ok_or("候选缺少起点。")?)?;
        let end = tick(item.get("end_s").ok_or("候选缺少终点。")?)?;
        if start >= end || end > duration {
            return Err("候选范围超出原片或起止顺序无效。".into());
        }
        let (start, end) = (rate.snap_nearest(start).min(limit), rate.snap_nearest(end).min(limit));
        if start < end {
            ranges.push(Interval { start, end });
        }
    }
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Interval> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut().filter(|r| range.start <= r.end) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    Ok(merged)
}

fn apply_result(app: &mut FilmcraftApp, result: &Value) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let generated = native::apply(&mut app.session, result)?;
        let count = generated.get("clipCount").and_then(Value::as_u64).unwrap_or(0);
        app.apex_ai.generated = Some(generated);
        app.apex_ai.error = None;
        app.apex_ai.stage = "completed".into();
        app.apex_ai.message = format!("已生成 {count} 段连续草稿。AI 原稿已保存，当前打开编辑副本。");
        app.ui.status = app.apex_ai.message.clone();
        app.ui.mode = crate::state::Mode::Edit;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, result);
        Err("本地 AI 原型只支持桌面版。".into())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use std::thread::JoinHandle;

    const MAX_LINE: u64 = 8 * 1024 * 1024;

    pub enum Outcome {
        Media(Value),
        Result { result: Value, cached: bool },
    }
    pub enum Event {
        Progress { stage: String, message: String, progress: Option<f32> },
        Finished(Result<Outcome, String>),
    }

    pub struct Worker {
        pub receiver: Option<Receiver<Event>>,
        child: Arc<Mutex<Option<Child>>>,
        cancel: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
        cancel_started: Option<std::time::Instant>,
    }

    fn terminate(child: &Arc<Mutex<Option<Child>>>, force: bool) {
        let mut slot = child.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(process) = slot.as_mut() {
            if process.try_wait().ok().flatten().is_some() {
                return;
            }
            #[cfg(unix)]
            if !force {
                // A numeric pid of our own child, no shell and no inherited command text.
                let sent = Command::new("/bin/kill")
                    .args(["-TERM", &process.id().to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                if sent.is_ok_and(|s| s.success()) {
                    return;
                }
            }
            let _ = process.kill();
        }
    }

    impl Worker {
        pub fn start(path: &str, audio: Option<u64>, ctx: egui::Context) -> Result<Self, String> {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let primary = std::fs::read_to_string(root.join(".git"))
                .ok()
                .and_then(|v| v.strip_prefix("gitdir: ").map(str::trim).map(PathBuf::from))
                .and_then(|p| p.parent()?.parent()?.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| root.clone());
            let model_python = primary.join("../apex-voice-detect/.venv-pretrainedsed/bin/python");
            let python = std::env::var_os("FILMCRAFT_APEX_PYTHON")
                .unwrap_or_else(|| if model_python.is_file() { model_python.into_os_string() } else { "python3".into() });
            let bridge = std::env::var_os("FILMCRAFT_APEX_BRIDGE").unwrap_or_else(|| root.join("integrations/apex-ai/bridge.py").into_os_string());
            let (sender, receiver) = sync_channel(64);
            let child = Arc::new(Mutex::new(None));
            let cancel = Arc::new(AtomicBool::new(false));
            let (worker_child, worker_cancel, path) = (child.clone(), cancel.clone(), path.to_string());
            let join = std::thread::Builder::new()
                .name("apex-ai-bridge".into())
                .spawn(move || {
                    let work = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        bridge_run(python, bridge, &path, audio, &worker_child, &worker_cancel, &sender, &ctx)
                    }));
                    let outcome = work.unwrap_or_else(|_| Err("AI 后台任务异常退出，当前项目未被替换。".into()));
                    terminate(&worker_child, true);
                    if let Some(mut process) = worker_child.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
                        let _ = process.wait();
                    }
                    let _ = sender.send(Event::Finished(outcome));
                    ctx.request_repaint();
                })
                .map_err(|e| format!("无法启动 AI 后台任务：{e}"))?;
            Ok(Self { receiver: Some(receiver), child, cancel, join: Some(join), cancel_started: None })
        }

        pub fn cancelled(&self) -> bool {
            self.cancel.load(Ordering::Relaxed)
        }
        pub fn cancel(&mut self) {
            self.cancel.store(true, Ordering::Relaxed);
            self.cancel_started.get_or_insert_with(std::time::Instant::now);
            terminate(&self.child, false);
        }
        pub fn tick_cancel(&self) {
            if self.cancel_started.is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(3)) {
                terminate(&self.child, true);
            }
        }
        pub fn finish(&mut self) {
            self.receiver.take();
            if let Some(thread) = self.join.take() {
                let _ = thread.join();
            }
        }
        pub fn shutdown(&mut self) {
            self.cancel();
            self.receiver.take();
            // Give the bridge its SIGTERM cleanup interval, then bound shutdown with kill+wait.
            for _ in 0..40 {
                if self.join.as_ref().is_none_or(JoinHandle::is_finished) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            terminate(&self.child, true);
            self.finish();
        }
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    fn bridge_run(
        python: std::ffi::OsString,
        bridge: std::ffi::OsString,
        path: &str,
        audio: Option<u64>,
        child: &Arc<Mutex<Option<Child>>>,
        cancel: &AtomicBool,
        sender: &SyncSender<Event>,
        ctx: &egui::Context,
    ) -> Result<Outcome, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("任务已取消。".into());
        }
        let mut command = Command::new(python);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        if std::env::var_os("APEX_SERVICE_ROOT").is_none() {
            let primary = std::fs::read_to_string(root.join(".git"))
                .ok()
                .and_then(|v| v.strip_prefix("gitdir: ").map(str::trim).map(PathBuf::from))
                .and_then(|p| p.parent()?.parent()?.parent().map(Path::to_path_buf))
                .unwrap_or(root);
            command.env("APEX_SERVICE_ROOT", primary.join("../apex-fps-mvp"));
            if std::env::var_os("APEX_MODEL_WORKSPACE").is_none() {
                command.env("APEX_MODEL_WORKSPACE", primary.join("../apex-voice-detect"));
            }
            if std::env::var_os("APEX_AI_DATA_DIR").is_none() {
                command.env("APEX_AI_DATA_DIR", primary.join("outputs/apex-ai/model-data"));
            }
        }
        command.arg(bridge).arg(if audio.is_some() { "analyze" } else { "probe" }).args(["--input", path]);
        command.args(["--parent-pid", &std::process::id().to_string()]);
        if let Some(audio) = audio {
            command.args(["--audio-stream", &audio.to_string()]);
        }
        let mut process = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(|e| format!("启动 AI 桥失败：{e}"))?;
        let stdout = process.stdout.take();
        *child.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(process);
        let stdout = stdout.ok_or("AI 桥没有输出通道。")?;
        if cancel.load(Ordering::Relaxed) {
            terminate(child, false);
        }
        let mut reader = BufReader::new(stdout);
        let mut outcome = None;
        let mut error = None;
        let mut line = Vec::new();
        let mut exhausted = true;
        for _ in 0..100_000 {
            line.clear();
            let read = Read::by_ref(&mut reader).take(MAX_LINE + 1).read_until(b'\n', &mut line).map_err(|e| format!("读取 AI 桥失败：{e}"))?;
            if read == 0 {
                exhausted = false;
                break;
            }
            if line.len() as u64 > MAX_LINE {
                return Err("AI 返回的数据过大。".into());
            }
            if cancel.load(Ordering::Relaxed) {
                continue;
            }
            let event: Value = serde_json::from_slice(&line).map_err(|e| format!("AI 桥返回无效数据：{e}"))?;
            match event.get("type").and_then(Value::as_str) {
                Some("progress") => {
                    let stage = event.get("stage").and_then(Value::as_str).unwrap_or("analyzing").chars().take(128).collect();
                    let message = event.get("message").and_then(Value::as_str).unwrap_or("AI 分析中…").chars().take(2048).collect();
                    let progress = event.get("progress").and_then(Value::as_f64).filter(|p| p.is_finite()).map(|p| p.clamp(0.0, 1.0) as f32);
                    let _ = sender.try_send(Event::Progress { stage, message, progress });
                    ctx.request_repaint();
                }
                Some("media") => outcome = Some(Outcome::Media(event.get("media").cloned().ok_or("AI 探测结果缺少媒体。")?)),
                Some("result") => {
                    outcome = Some(Outcome::Result {
                        result: event.get("result").cloned().ok_or("AI 返回缺少分析结果。")?,
                        cached: event.get("cached").and_then(Value::as_bool).unwrap_or(false),
                    })
                }
                Some("error") => error = Some(event.get("message").and_then(Value::as_str).unwrap_or("AI 分析失败。 ").chars().take(4096).collect::<String>()),
                _ => return Err("AI 桥返回了不支持的消息类型。".into()),
            }
        }
        if exhausted {
            return Err("AI 消息数量超出原型支持范围。".into());
        }
        let status = loop {
            let status = child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
                .ok_or("AI 进程丢失。")?
                .try_wait()
                .map_err(|e| format!("等待 AI 进程失败：{e}"))?;
            if let Some(status) = status {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        };
        if let Some(error) = error {
            return Err(error);
        }
        if !status.success() {
            return Err(format!("AI 桥退出失败：{status}"));
        }
        outcome.ok_or_else(|| "AI 进程已结束，但没有返回结果。".into())
    }

    fn new_folder(s: &Session) -> Result<PathBuf, String> {
        let root = std::env::var_os("FILMCRAFT_APEX_PROJECTS_DIR")
            .map(PathBuf::from)
            .or_else(|| s.prefs_path.as_ref().and_then(|p| p.parent()).map(|p| p.join("apex-ai-projects")))
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../outputs/apex-ai/projects"));
        if !root.is_absolute() {
            return Err("AI 工程保存目录必须是绝对路径。".into());
        }
        std::fs::create_dir_all(&root).map_err(|e| format!("无法创建 AI 工程目录：{e}"))?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        for suffix in 0..16 {
            let folder = root.join(format!("draft-{now}-{}-{suffix}", std::process::id()));
            match std::fs::create_dir(&folder) {
                Ok(()) => return Ok(folder),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("无法创建独立草稿目录：{e}")),
            }
        }
        Err("无法分配独立草稿目录。".into())
    }

    fn source_item(s: &mut Session, path: &str) -> filmcraft_engine::Result<ItemId> {
        let existing = s.project.items.iter().find_map(|(id, item)| {
            let media = item.as_media()?;
            matches!(&media.media, MediaRef::File { path: p } if p == path).then_some(*id)
        });
        if let Some(id) = existing {
            return Ok(id);
        }
        let imported = s.execute("file.import", json!({"paths":[path]}))?;
        if imported.get("errors").and_then(Value::as_array).is_some_and(|v| !v.is_empty()) {
            return Err(EngineError::Other(format!("导入原片失败：{}", imported.get("errors").unwrap_or(&Value::Null))));
        }
        imported
            .get("items")
            .and_then(Value::as_array)
            .and_then(|v| v.first())
            .and_then(Value::as_u64)
            .map(ItemId)
            .ok_or_else(|| EngineError::Other("编辑器未返回导入素材标识。".into()))
    }

    pub fn apply(s: &mut Session, result: &Value) -> Result<Value, String> {
        let media = result.get("media").ok_or("AI 结果缺少媒体信息。")?;
        let path = media.pointer("/source/path").and_then(Value::as_str).ok_or("AI 结果缺少原片路径。")?;
        let source = Path::new(path);
        if !source.is_absolute() || !source.is_file() {
            return Err("AI 原片已移动或不可用，请重新分析。".into());
        }
        let stat = std::fs::metadata(source).map_err(|e| format!("无法核对原片：{e}"))?;
        let modified =
            stat.modified().map_err(|e| format!("无法核对原片时间：{e}"))?.duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        if media.pointer("/source/size_bytes").and_then(Value::as_u64) != Some(stat.len())
            || media.pointer("/source/mtime_ns").and_then(Value::as_u64).map(u128::from) != Some(modified)
        {
            return Err("分析完成后原片已变化，请重新读取并分析。".into());
        }
        // The model's source time is relative to the first video frame. Nonzero video origins
        // need a container-specific mapping to FilmCraft's earliest-playable-stream origin.
        if media.pointer("/video/start_time_s").and_then(Value::as_f64) != Some(0.0) {
            return Err("结果已保留。本原型尚未验证视频非零起始时间的映射，未自动写入草稿。".into());
        }
        let audio_tracks = tracks(media);
        let selected_audio = media.pointer("/selected_audio/stream_index").and_then(Value::as_u64).ok_or("AI 结果缺少已确认音轨。")?;
        if !audio_tracks.iter().any(|t| t["stream_index"].as_u64() == Some(selected_audio)) {
            return Err("分析结果的所选音轨不在原录像中。".into());
        }
        let result_id = result.get("result_id").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("分析结果缺少标识。")?;
        let events = result.get("events").and_then(Value::as_array).ok_or("分析结果缺少事件列表。")?;
        if events.len() > MAX_EVENTS {
            return Err("AI 事件数量超出原型支持范围。".into());
        }
        let folder = new_folder(s)?;
        let original = folder.join("ai-original.fcproj");
        let editable = folder.join("edit-copy.fcproj");
        let original_path = original.to_str().ok_or("工程路径不是有效文本。")?;
        let edit_path = editable.to_str().ok_or("工程路径不是有效文本。")?;
        let original_session_path = s.path.clone();
        let saved_revision = s.saved_revision;
        // Child commands normally send intermediate snapshots to Auto Save, which may also save
        // the current project. Keep those snapshots away from the original project path.
        let persistence = s.persistence.take();
        let imported = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.edit_group("AI 生成连续草稿", |s| {
                let source_id = source_item(s, path)?;
                let item = s.project.item(source_id).ok_or_else(|| EngineError::Other("原片素材不存在。".into()))?;
                let native_media = item.as_media().ok_or_else(|| EngineError::Other("AI 草稿需要文件素材。".into()))?;
                let video = native_media.info.video.clone().ok_or_else(|| EngineError::Other("原片没有可用视频。".into()))?;
                let audio = native_media.info.audio_streams.first().cloned().ok_or_else(|| EngineError::Other("编辑器未识别原片音轨。".into()))?;
                if native_media.offline || native_media.proxy.is_some() || native_media.interpret != Default::default() {
                    return Err(EngineError::Other("当前素材有离线、代理或自定义解读设置；本原型仅支持原始默认素材，分析结果已保留。".into()));
                }
                if video.frame_rate.num <= 0 || video.frame_rate.den <= 0 || video.width == 0 || video.height == 0 || audio.sample_rate == 0 {
                    return Err(EngineError::Other("原片尺寸、帧率或音频采样率无效。".into()));
                }
                if media.pointer("/video/width").and_then(Value::as_u64) != Some(u64::from(video.width))
                    || media.pointer("/video/height").and_then(Value::as_u64) != Some(u64::from(video.height))
                {
                    return Err(EngineError::Other("编辑器和分析服务的视频尺寸不一致。".into()));
                }
                let mix = if native_media.info.audio_streams.iter().all(|a| a.channels == 1) { "Mono" } else { "Stereo" };
                // The same checked container-index/native-ordinal map used by
                // recognition and audition gates draft placement of every stream.
                s.execute("recognition.mapAudio", json!({"item":source_id.0,"media":media}))?;
                let rate = video.frame_rate;
                let duration = tick(media.get("duration_s").ok_or_else(|| EngineError::Other("缺少原片时长。".into()))?).map_err(EngineError::Other)?;
                let ranges = intervals(result, rate, duration).map_err(EngineError::Other)?;
                if ranges.is_empty() {
                    return Err(EngineError::Other("没有可生成草稿的枪声候选；原始分析结果已保留。".into()));
                }
                let sequence = s.execute(
                    "file.newSequence",
                    json!({"name":"AI 原始草稿", "width":video.width, "height":video.height,
                "fps":rate.num as f64 / rate.den as f64, "sampleRate":audio.sample_rate, "video":1, "audio":1,
                "mix":mix}),
                )?;
                let sequence_id = sequence.get("sequence").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("没有生成序列。".into()))?;
                s.execute("source.open", json!({"item":source_id.0}))?;
                let mut marker_count = 0usize;
                for event in events {
                    let start = tick(event.get("start_s").ok_or_else(|| EngineError::Other("事件缺少起点。".into()))?).map_err(EngineError::Other)?;
                    let end = tick(event.get("end_s").ok_or_else(|| EngineError::Other("事件缺少终点。".into()))?).map_err(EngineError::Other)?;
                    if start >= end || end > duration {
                        return Err(EngineError::Other("AI 事件范围无效。".into()));
                    }
                    let comment = json!({"apexAiResult":result_id,"predictionId":result.get("prediction_id"),"event":event}).to_string();
                    let already = s.project.item(source_id).and_then(|i| i.as_media()).is_some_and(|m| m.markers.iter().any(|m| m.comment == comment));
                    if !already {
                        let start = rate.snap_nearest(start).min(rate.snap(duration));
                        let end = rate.snap_nearest(end).min(rate.snap(duration));
                        s.execute(
                            "markers.add",
                            json!({"target":"source", "time":start.0, "durationFrames":rate.frame_at(end-start).max(0),
                        "name":"AI 枪声候选", "comment":comment, "color":"Yellow"}),
                        )?;
                    }
                    marker_count = marker_count.saturating_add(1);
                }
                let mut cursor = Tick::ZERO;
                let mut mapping = Vec::new();
                for range in &ranges {
                    let duration = range.end - range.start;
                    let placed = s.execute(
                        "timeline.place",
                        json!({"item":source_id.0,"track":"V1","audioTrack":"A1", "time":cursor.0,
                    "sourceIn":range.start.0,"duration":duration.0,"insert":false,"video":true,"audio":true}),
                    )?;
                    mapping.push(json!({"sourceStart":range.start.0,"sourceEnd":range.end.0,"sourceStartSeconds":range.start.seconds(),
                    "sourceEndSeconds":range.end.seconds(),"timelineStart":cursor.0,"timelineEnd":(cursor+duration).0,"clips":placed.get("clips")}));
                    cursor += duration;
                }
                let metadata = json!({"result":result,"sourceItem":source_id.0,"sequence":sequence_id,"ranges":mapping});
                let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| EngineError::Other(e.to_string()))?;
                let mut evidence = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(folder.join("ai-evidence.json"))
                    .map_err(|e| EngineError::Other(format!("保存 AI 证据失败：{e}")))?;
                evidence.write_all(&bytes).and_then(|()| evidence.sync_all()).map_err(|e| EngineError::Other(format!("保存 AI 证据失败：{e}")))?;
                s.execute("file.saveCopy", json!({"path":original_path}))?;
                s.execute("sequence.settings", json!({"name":"AI 编辑副本"}))?;
                s.execute("file.saveAs", json!({"path":edit_path}))?;
                s.execute("playhead.set", json!({"time":0}))?;
                Ok(json!({"resultId":result_id,"sourceItem":source_id.0,"sequence":sequence_id,
                "clipCount":ranges.len(),"eventCount":marker_count,"durationSeconds":cursor.seconds(),"durationFrames":rate.frame_at(cursor),
                "frameRate":{"num":rate.num,"den":rate.den},"ranges":mapping,
                "originalProject":original_path,"editProject":edit_path,"evidence":folder.join("ai-evidence.json")}))
            })
        }));
        s.persistence = persistence;
        let output = imported.map_err(|_| "生成草稿异常，已恢复原工程，分析结果已保留。".to_string()).and_then(|result| result.map_err(|e| e.to_string()));
        if output.is_err() {
            s.path = original_session_path;
            s.saved_revision = saved_revision;
        }
        s.sync_persistence();
        output
    }
}
