//! Native analysis adapter. Completion only attaches evidence; assembly requires an explicit action.

use std::sync::Arc;

use filmcraft_engine::Session;
use filmcraft_project::{AnalysisStatus as Status, ItemId, MediaRef};
use serde_json::{Value, json};

use crate::FilmcraftApp;

#[path = "association_ui.rs"]
mod association;
#[path = "analysis_queue_ui.rs"]
mod queue;

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub reference_item: Option<ItemId>,
    pub collapsed: bool,
    pub gunshot_collapsed: bool,
    pub voice_collapsed: bool,
    pub states: std::collections::BTreeMap<ItemId, String>,
    pub recovery: std::collections::BTreeMap<ItemId, Value>,
    validation_pending: std::collections::VecDeque<ItemId>,
    incarnation: u64,
    selected_job: Option<u64>,
    active: Option<queue::Active>,
    queue: Vec<Value>,
    job_progress: std::collections::BTreeMap<u64, (String, Option<f32>)>,
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
    include_voice: bool,
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
            "open": self.open, "busy": self.busy(), "path": self.path,"queue":self.queue,"selectedJob":self.selected_job,"activeJob":self.active.as_ref().filter(|a|a.validation.is_none()).map(|a|a.id),"activeAttempt":self.active.as_ref().filter(|a|a.validation.is_none()).map(|a|a.attempt),
            "sourceItem":self.reference_item.map(|i|i.0),"sources":self.states.iter().map(|(id,status)|json!({"item":id.0,"status":status})).collect::<Vec<_>>(),
            "audioStream": self.audio_stream, "auditionedStreams":self.auditioned, "media": self.media,
            "stage": self.stage, "message": self.message, "progress": self.progress,
            "error": self.error, "cached": self.cached, "pendingResult": self.pending.is_some(),
            "resultId": self.pending.as_ref().and_then(|r| r.get("result_id")),
            "generated": self.generated,
            "includeVoice": self.include_voice,
            "associations":self.recovery.iter().map(|(_,v)|v.clone()).collect::<Vec<_>>(),
            "activeValidation":self.active.as_ref().and_then(|a|a.validation.as_ref().map(|(item,id)|json!({"item":item.0,"resultId":id}))),
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
        app.session.project.item(*id).and_then(|i| i.as_media()).is_some_and(|m| match &m.media {
            MediaRef::File { path: p } => match (std::fs::canonicalize(p), std::fs::canonicalize(path)) {
                (Ok(a), Ok(b)) => a == b,
                _ => p == path,
            },
            _ => false,
        })
    })
}

fn assembly_source(app: &FilmcraftApp) -> Option<ItemId> {
    source_for_path(app, &app.apex_ai.path).or(app.session.state.source_item).or(app.apex_ai.reference_item)
}

/// UI/control commands share this entry point. Only explicit assembly opens a new editing copy.
pub fn invoke(app: &mut FilmcraftApp, ctx: &egui::Context, id: &str, params: &Value) -> Result<Value, String> {
    match id {
        "apexAi.open" => queue::open(app, params)?,
        "apexAi.close" => {
            app.apex_ai.open = false;
            app.clear_source_audition();
        }
        "apexAi.audition" => {
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
        "apexAi.probe" => queue::request(app, params, false)?,
        "apexAi.analyze" => queue::request(app, params, true)?,
        "apexAi.cancel" => queue::cancel(app, params)?,
        "apexAi.retry" => queue::retry(app, params)?,
        "apexAi.validate" => association::request(app, params)?,
        "apexAi.cancelValidation" =>
        {
            #[cfg(not(target_arch = "wasm32"))]
            if app.apex_ai.active.as_ref().is_some_and(|a| a.validation.is_some()) {
                if let Some(worker) = &mut app.apex_ai.worker {
                    worker.cancel();
                }
            }
        }
        "apexAi.apply" => {
            if app.apex_ai.busy() {
                return Err("请等待当前任务停止。".into());
            }
            let source = match params.get("item") {
                Some(v) => v.as_u64().map(ItemId),
                None => assembly_source(app),
            }
            .ok_or("请先打开要组稿的原片。")?;
            let mut assembly = params.clone();
            assembly["item"] = json!(source.0);
            if assembly.get("categories").is_none() {
                assembly["categories"] = if app.apex_ai.include_voice { json!(["gunshot", "voice"]) } else { json!(["gunshot"]) };
            }
            app.clear_source_audition();
            apply_result(app, &assembly)?;
        }
        "apexAi.state" | "apexAi.inspect" => {}
        _ => return Err("未知 AI 剪辑命令。".into()),
    }
    queue::refresh(app);
    ctx.request_repaint();
    Ok(app.apex_ai.snapshot())
}

/// Advance one project-bound analysis worker independently of the dialog selection.
pub fn poll(app: &mut FilmcraftApp, ctx: &egui::Context) {
    queue::poll(app, ctx);
}

fn fail(app: &mut FilmcraftApp, message: String) {
    app.apex_ai.pending = None;
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
        ui.add_enabled_ui(true, |ui| {
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
            let analyze = ui.add_enabled(ready && queue::can_confirm(app), egui::Button::new("确认音轨并分析声音"));
            app.auto.add("apexAi.analyze", analyze.rect, "确认音轨并分析声音");
            if analyze.clicked() {
                action = Some(("apexAi.analyze", json!({})));
            }
        });
        if let Some(item) = app.apex_ai.reference_item {
            if let Some(state) = app.apex_ai.recovery.get(&item).cloned() {
                ui.separator();
                ui.label(state["reason"].as_str().unwrap_or("分析关联待核对"));
                ui.small("已保存的事件、人工范围与剪辑保留；只读核对不会重新分析。");
                ui.horizontal(|ui| {
                    let check = ui.button("核对已存分析");
                    app.auto.add("apexAi.validate", check.rect, "只读核对已保存分析关联");
                    if check.clicked() {
                        action = Some(("apexAi.validate", json!({"item":item.0})));
                    }
                    let retry = ui.button("重新分析此原片");
                    app.auto.add("apexAi.retryAssociation", retry.rect, "主动重新分析此原片，保留人工工作");
                    if retry.clicked() {
                        action = Some(("apexAi.retry", json!({"item":item.0})));
                    }
                });
            }
        }
        ui.separator();
        ui.label("分析队列 · 按导入顺序处理就绪项目，待选音轨不阻塞后续素材");
        for task in app.apex_ai.queue.clone() {
            let Some(id) = task["id"].as_u64() else { continue };
            let item = task["item"].as_u64().map(ItemId);
            let name = task["source_path"]
                .as_str()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .or_else(|| item.and_then(|i| app.session.project.item(i)).map(|i| i.name.clone()))
                .unwrap_or_else(|| "已移除素材".into());
            let status = task["status"].as_str().unwrap_or_default();
            ui.horizontal(|ui| {
                let open = ui.button(format!("#{id} {name} · {}", queue::label(status)));
                app.auto.add(&format!("apexAi.queue.{id}.open"), open.rect, &format!("打开分析任务 {id}"));
                if open.clicked() {
                    action = Some(("apexAi.open", json!({"job":id})));
                }
                if Status::from(status).is_pending() {
                    let cancel = ui.button("取消");
                    app.auto.add(&format!("apexAi.queue.{id}.cancel"), cancel.rect, "取消此分析任务");
                    if cancel.clicked() {
                        action = Some(("apexAi.cancel", json!({"job":id})));
                    }
                } else {
                    let retry = ui.button("重试");
                    app.auto.add(&format!("apexAi.queue.{id}.retry"), retry.rect, "主动重试此分析任务");
                    if retry.clicked() {
                        action = Some(("apexAi.retry", json!({"job":id})));
                    }
                }
            });
            if let Some(error) = task["error"].as_str() {
                ui.small(error);
            }
            if let Some((message, progress)) = app.apex_ai.job_progress.get(&id) {
                if Status::from(status).has_worker() {
                    ui.small(message);
                    if let Some(progress) = progress {
                        ui.add(egui::ProgressBar::new(*progress));
                    }
                }
            }
        }
        if busy {
            match app.apex_ai.progress {
                Some(p) => {
                    ui.add(egui::ProgressBar::new(p).show_percentage());
                }
                None => {
                    ui.spinner();
                }
            }
            let cancel = ui.button("取消当前运行任务");
            let validation = app.apex_ai.active.as_ref().is_some_and(|a| a.validation.is_some());
            app.auto.add(if validation { "apexAi.cancelValidation" } else { "apexAi.cancel" }, cancel.rect, "取消当前后台任务");
            if cancel.clicked() {
                action = if validation {
                    Some(("apexAi.cancelValidation", json!({})))
                } else {
                    Some(("apexAi.cancel", json!({"job":app.apex_ai.active.as_ref().map(|a|a.id)})))
                };
            }
        }
        ui.label(&app.apex_ai.message);
        if let Some(error) = &app.apex_ai.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if !busy {
            ui.separator();
            ui.label("按当前原片的待剪范围组稿（默认枪声）");
            let voice = ui.checkbox(&mut app.apex_ai.include_voice, "包含人声待剪范围");
            app.auto.add("apexAi.includeVoice", voice.rect, "组稿包含人声待剪范围");
            let source = assembly_source(app);
            let apply = ui.add_enabled(source.is_some(), egui::Button::new("按当前范围组稿"));
            app.auto.add("apexAi.apply", apply.rect, "按当前范围组稿");
            ui.small("采用当前人工调整，合并重叠范围；另建组稿原稿与编辑副本，保留此前剪辑。不会重新分析。");
            if apply.clicked() {
                action = Some(("apexAi.apply", json!({"item":source.map(|id|id.0),"projectToken":app.session.recognition_jobs.incarnation})));
            }
        }
        if let Some(generated) = &app.apex_ai.generated {
            ui.separator();
            ui.label("组稿基准采用当时的待剪范围，可能包含人工调整");
            if let Some(path) = generated.get("originalProject").and_then(Value::as_str) {
                ui.small(format!("组稿原稿：{path}"));
            }
            if let Some(path) = generated.get("editProject").and_then(Value::as_str) {
                ui.small(format!("对应编辑副本：{path}"));
            }
            if let Some(path) = generated.get("preservedProject").and_then(Value::as_str) {
                ui.small(format!("组稿前工作：{path}"));
            }
            ui.small("可在时间线删段、改边界、补入原片；完成后使用原生导出。");
            if app.session.path.as_deref() == generated["originalProject"].as_str() {
                ui.label("当前查看组稿原稿；保存受保护，可另存副本编辑。");
            } else {
                ui.small("当前工程是独立编辑副本；后续修改不会改写组稿原稿。");
            }
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
        if command == "apexAi.apply" {
            app.apex_ai.error = Some(e.clone());
            app.apex_ai.stage = "assemblyFailed".into();
            app.apex_ai.message = format!("组稿失败：{e}。已有剪辑与识别结果已保留。");
            app.ui.status = app.apex_ai.message.clone();
        } else {
            fail(app, e);
        }
    }
}

fn apply_result(app: &mut FilmcraftApp, params: &Value) -> Result<(), String> {
    let generated = app.session.execute("selection.assemble", params.clone()).map_err(|e| e.to_string())?;
    if generated["status"] == "noRanges" {
        app.apex_ai.message = generated["message"].as_str().unwrap_or("没有可组稿范围，已有剪辑未改变。").into();
    } else {
        let count = generated["clipCount"].as_u64().unwrap_or(0);
        app.apex_ai.generated = Some(generated);
        app.apex_ai.stage = "completed".into();
        app.apex_ai.message = format!("已按当前范围生成 {count} 段。组稿原稿与此前工作已保存，当前打开独立编辑副本。");
        app.ui.mode = crate::state::Mode::Edit;
    }
    app.apex_ai.error = None;
    app.ui.status = app.apex_ai.message.clone();
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use std::thread::JoinHandle;

    const MAX_LINE: u64 = 8 * 1024 * 1024;

    pub enum Outcome {
        Validation(Value),
        Media(Value),
        Result { result: Value, cached: bool },
    }
    pub struct Failure {
        pub code: Option<String>,
        pub message: String,
    }
    impl From<String> for Failure {
        fn from(message: String) -> Self {
            Self { code: None, message }
        }
    }
    impl From<&str> for Failure {
        fn from(message: &str) -> Self {
            message.to_string().into()
        }
    }
    pub enum Event {
        Progress { stage: String, message: String, progress: Option<f32> },
        Finished(Result<Outcome, Failure>),
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
            Self::start_request(path, audio, None, ctx)
        }
        pub fn start_validation(path: &str, audio: u64, result_id: &str, ctx: egui::Context) -> Result<Self, String> {
            Self::start_request(path, Some(audio), Some(result_id.to_string()), ctx)
        }
        fn start_request(path: &str, audio: Option<u64>, validation: Option<String>, ctx: egui::Context) -> Result<Self, String> {
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
                        bridge_run(python, bridge, &path, audio, validation.as_deref(), &worker_child, &worker_cancel, &sender, &ctx)
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
        validation: Option<&str>,
        child: &Arc<Mutex<Option<Child>>>,
        cancel: &AtomicBool,
        sender: &SyncSender<Event>,
        ctx: &egui::Context,
    ) -> Result<Outcome, Failure> {
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
        command
            .arg(bridge)
            .arg(if validation.is_some() {
                "validate"
            } else if audio.is_some() {
                "analyze"
            } else {
                "probe"
            })
            .args(["--input", path]);
        if let Some(id) = validation {
            command.args(["--result-id", id]);
        }
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
                Some("validation") => outcome = Some(Outcome::Validation(event.clone())),
                Some("media") => outcome = Some(Outcome::Media(event.get("media").cloned().ok_or("AI 探测结果缺少媒体。")?)),
                Some("result") => {
                    outcome = Some(Outcome::Result {
                        result: event.get("result").cloned().ok_or("AI 返回缺少分析结果。")?,
                        cached: event.get("cached").and_then(Value::as_bool).unwrap_or(false),
                    })
                }
                Some("error") => {
                    error = Some(Failure {
                        code: event["code"].as_str().map(|c| c.chars().take(128).collect()),
                        message: event.get("message").and_then(Value::as_str).unwrap_or("AI 分析失败。").chars().take(4096).collect(),
                    })
                }
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
            return Err(format!("AI 桥退出失败：{status}").into());
        }
        outcome.ok_or_else(|| "AI 进程已结束，但没有返回结果。".into())
    }
}
