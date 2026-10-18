//! A serial, project-bound queue. Commands mutate metadata without occupying edit undo.
use crate::{
    EngineError, Result, Session,
    commands::{CommandSpec, always, bad},
};
use filmcraft_project::{AnalysisStatus as Status, AnalysisTask, ItemId, MediaRef};
use serde_json::{Value, json};
use std::sync::Arc;

fn error(message: impl Into<String>) -> EngineError {
    bad("analysis", message)
}
fn id(p: &Value) -> Result<u64> {
    p["job"].as_u64().ok_or_else(|| error("job is required"))
}
fn index(tasks: &[AnalysisTask], p: &Value) -> Result<usize> {
    let id = id(p)?;
    tasks.iter().position(|t| t.id == id).ok_or_else(|| error("分析任务不存在"))
}
fn current(tasks: &[AnalysisTask], p: &Value, status: Status) -> Result<usize> {
    let i = index(tasks, p)?;
    if p["attempt"].as_u64() != Some(u64::from(tasks[i].attempt)) || tasks[i].status != status {
        return Err(error("任务已取消、重试或状态已变化，忽略迟到响应"));
    }
    Ok(i)
}
fn value(task: &AnalysisTask) -> Value {
    serde_json::to_value(task).unwrap_or(Value::Null)
}
fn persist(s: &mut Session, tasks: Vec<AnalysisTask>) {
    let tasks = Arc::new(tasks);
    Arc::make_mut(&mut s.project).analysis_queue = tasks.clone();
    // Operational queue metadata is not an editing action. Keep it current in
    // history snapshots too, so undoing a trim cannot resurrect a cancelled job.
    for (_, snapshot) in s.history.undo.iter_mut().chain(s.history.redo.iter_mut()) {
        Arc::make_mut(snapshot).analysis_queue = tasks.clone();
    }
    s.analysis_metadata_updates = s.analysis_metadata_updates.saturating_add(1);
    s.bump();
}
fn availability(s: &Session, item: ItemId) -> (String, Option<String>) {
    let Some(media) = s.project.item(item).and_then(|i| i.as_media()) else { return (String::new(), Some("素材不存在或不是文件录像".into())) };
    let MediaRef::File { path } = &media.media else { return (String::new(), Some("请选择本地原录像".into())) };
    let reason = if media.offline || !std::path::Path::new(path).is_file() {
        Some("原片离线或已移动".into())
    } else if media.proxy.is_some() || media.interpret != Default::default() {
        Some("当前代理或自定义素材解读尚不支持分析".into())
    } else if media.info.video.is_none() {
        Some("素材没有可分析的视频".into())
    } else if media.info.audio_streams.is_empty() {
        Some("原片没有可分析音轨".into())
    } else {
        None
    };
    (std::fs::canonicalize(path).ok().and_then(|p| p.to_str().map(str::to_string)).unwrap_or_else(|| path.clone()), reason)
}
fn stamp(path: &str) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = u64::try_from(meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos()).ok()?;
    Some((meta.len(), modified))
}
fn same_source(task: &AnalysisTask, path: &str, identity: Option<(u64, u64)>) -> bool {
    task.source_path == path
        && match identity {
            Some((size, time)) => task.source_size_bytes == Some(size) && task.source_mtime_ns == Some(time),
            None => task.source_size_bytes.is_none() && task.source_mtime_ns.is_none(),
        }
}
fn check_origin(s: &Session, task: &AnalysisTask) -> Result<()> {
    let (path, reason) = availability(s, task.item);
    if let Some(reason) = reason {
        return Err(error(reason));
    }
    if !same_source(task, &path, stamp(&path)) {
        return Err(error("原录像来源身份已变化；旧任务不会接管被替换或重用编号的素材"));
    }
    Ok(())
}

fn enqueue(s: &mut Session, p: &Value) -> Result<Value> {
    let item = ItemId(p["item"].as_u64().ok_or_else(|| error("item is required"))?);
    if s.project.item(item).is_none() {
        return Err(error("素材不存在"));
    }
    let (path, mut reason) = availability(s, item);
    let identity = stamp(&path);
    if reason.is_none() && identity.is_none() {
        reason = Some("无法读取原录像身份".into());
    }
    if let Some(task) = s.project.analysis_queue.iter().rev().find(|t| t.item == item && same_source(t, &path, identity)) {
        return Ok(value(task));
    }
    if s.project.analysis_queue.len() >= 4096 {
        return Err(error("分析队列已满"));
    }
    let task = AnalysisTask {
        id: s.project.analysis_queue.iter().map(|t| t.id).max().unwrap_or(0).checked_add(1).ok_or_else(|| error("任务编号已耗尽"))?,
        item,
        attempt: 1,
        status: if reason.is_some() { Status::Unavailable } else { Status::Queued },
        source_path: path,
        source_size_bytes: identity.map(|i| i.0),
        source_mtime_ns: identity.map(|i| i.1),
        error: reason,
        media: None,
        audio_stream: None,
        result_id: None,
        previous_status: None,
        token: None,
    };
    let output = value(&task);
    let mut tasks = (*s.project.analysis_queue).clone();
    tasks.push(task);
    persist(s, tasks);
    Ok(output)
}
fn start(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = current(&tasks, p, Status::Queued)?;
    if tasks.iter().position(|t| t.status == Status::Queued) != Some(i) {
        return Err(error("请按就绪队列顺序领取任务"));
    }
    if tasks.iter().any(|t| t.status.has_worker()) {
        return Err(error("已有一个分析进程运行，请等待"));
    }
    if let Err(reason) = check_origin(s, &tasks[i]) {
        tasks[i].status = Status::Unavailable;
        tasks[i].error = Some(reason.to_string());
    } else if let Some(media) = tasks[i].media.clone() {
        match s.execute("recognition.begin", json!({"item":tasks[i].item.0,"audioStream":tasks[i].audio_stream,"media":media})) {
            Ok(origin) => {
                tasks[i].token = origin["token"].as_u64();
                tasks[i].status = Status::Running;
            }
            Err(e) => {
                tasks[i].status = Status::Unavailable;
                tasks[i].error = Some(e.to_string());
            }
        }
    } else {
        tasks[i].status = Status::Probing;
    }
    let mut output = value(&tasks[i]);
    output["token"] = json!(tasks[i].token);
    persist(s, tasks);
    Ok(output)
}
fn probed(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = current(&tasks, p, Status::Probing)?;
    let media = p.get("media").ok_or_else(|| error("media is required"))?;
    match check_origin(s, &tasks[i]).and_then(|_| s.execute("recognition.mapAudio", json!({"item":tasks[i].item.0,"media":media}))) {
        Err(e) => {
            tasks[i].status = Status::Unavailable;
            tasks[i].error = Some(e.to_string());
        }
        Ok(_) => {
            let tracks = media["audio_tracks"].as_array().ok_or_else(|| error("音轨列表无效"))?;
            tasks[i].media = Some(media.clone());
            tasks[i].audio_stream = if tracks.len() == 1 { tracks[0]["stream_index"].as_u64() } else { None };
            tasks[i].status = if tracks.len() == 1 { Status::Queued } else { Status::WaitingAudio };
        }
    }
    let output = value(&tasks[i]);
    persist(s, tasks);
    Ok(output)
}
fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = current(&tasks, p, Status::WaitingAudio)?;
    let audio = p["audioStream"].as_u64().ok_or_else(|| error("audioStream is required"))?;
    check_origin(s, &tasks[i])?;
    let media = tasks[i].media.as_ref().ok_or_else(|| error("请重新读取音轨"))?;
    let mapping = s.execute("recognition.mapAudio", json!({"item":tasks[i].item.0,"media":media}))?;
    if !mapping["audioMapping"].as_array().is_some_and(|a| a.iter().any(|r| r["containerStream"].as_u64() == Some(audio))) {
        return Err(error("所选音轨不存在"));
    }
    tasks[i].audio_stream = Some(audio);
    tasks[i].status = Status::Queued;
    let output = value(&tasks[i]);
    persist(s, tasks);
    Ok(output)
}
fn cancel(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = index(&tasks, p)?;
    if !tasks[i].status.can_cancel() {
        return Ok(value(&tasks[i]));
    }
    if let Some(token) = tasks[i].token.take() {
        s.execute("recognition.cancel", json!({"token":token}))?;
    }
    tasks[i].status = Status::Cancelled;
    tasks[i].error = Some("用户已取消；迟到结果不会写入工程".into());
    let output = value(&tasks[i]);
    persist(s, tasks);
    Ok(output)
}
fn fail(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = index(&tasks, p)?;
    if p["attempt"].as_u64() != Some(u64::from(tasks[i].attempt)) || !tasks[i].status.is_pending() {
        return Err(error("已过期的分析失败响应"));
    }
    let message = p["error"].as_str().filter(|v| !v.is_empty()).ok_or_else(|| error("失败原因缺失"))?;
    if let Some(token) = tasks[i].token.take() {
        s.execute("recognition.cancel", json!({"token":token}))?;
    }
    tasks[i].error = Some(message.chars().take(8192).collect());
    tasks[i].status = match p["code"].as_str() {
        Some(
            "backend_unavailable"
            | "backend_import_failed"
            | "model_runtime_unavailable"
            | "runtime_unavailable"
            | "core_runtime_unavailable"
            | "invalid_path"
            | "source_changed",
        ) => Status::Unavailable,
        Some("task_cancelled") => Status::Cancelled,
        Some("service_interrupted") => Status::Interrupted,
        _ => Status::Failed,
    };
    let output = value(&tasks[i]);
    persist(s, tasks);
    Ok(output)
}
fn finish(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = current(&tasks, p, Status::Running)?;
    let token = tasks[i].token.ok_or_else(|| error("任务没有有效接收令牌"))?;
    let result = p.get("result").ok_or_else(|| error("result is required"))?;
    let origin = check_origin(s, &tasks[i]);
    let source_valid = origin.is_ok();
    match origin.and_then(|_| s.execute("recognition.attach", json!({"token":token,"result":result}))) {
        Ok(_) => {
            tasks[i].status = Status::Completed;
            tasks[i].result_id = result["result_id"].as_str().map(str::to_string);
            tasks[i].error = None;
        }
        Err(e) => {
            tasks[i].status = if source_valid { Status::Failed } else { Status::Unavailable };
            tasks[i].error = Some(e.to_string());
            s.execute("recognition.cancel", json!({"token":token}))?;
        }
    }
    tasks[i].token = None;
    let output = value(&tasks[i]);
    persist(s, tasks);
    Ok(output)
}
fn retry(s: &mut Session, p: &Value) -> Result<Value> {
    let mut tasks = (*s.project.analysis_queue).clone();
    let i = index(&tasks, p)?;
    if tasks[i].status == Status::Queued {
        return Ok(value(&tasks[i]));
    }
    if tasks[i].status.is_pending() {
        return Err(error("请先取消当前请求再重试"));
    }
    tasks[i].attempt = tasks[i].attempt.checked_add(1).ok_or_else(|| error("重试次数已耗尽"))?;
    let (path, mut reason) = availability(s, tasks[i].item);
    if !tasks[i].source_path.is_empty() && tasks[i].source_path != path {
        tasks[i].status = Status::Unavailable;
        tasks[i].error = Some("原素材已移除或被替换；请对新素材重新入队".into());
        let output = value(&tasks[i]);
        persist(s, tasks);
        return Ok(output);
    }
    let identity = stamp(&path);
    if reason.is_none() && identity.is_none() {
        reason = Some("无法读取原录像身份".into());
    }
    tasks[i].source_path = path;
    tasks[i].source_size_bytes = identity.map(|i| i.0);
    tasks[i].source_mtime_ns = identity.map(|i| i.1);
    tasks[i].status = if reason.is_some() { Status::Unavailable } else { Status::Queued };
    tasks[i].media = None;
    tasks[i].audio_stream = None;
    tasks[i].error = reason;
    tasks[i].token = None;
    tasks[i].previous_status = None;
    let output = value(&tasks[i]);
    persist(s, tasks);
    Ok(output)
}
pub fn commands() -> Vec<CommandSpec> {
    let make = |id, label, run| CommandSpec { id, label, menu: &[], shortcut: None, params: "{}", enabled: always, run, journal: false };
    vec![
        make("analysis.checkOrigin", "核对队列来源", |s, p| {
            let i = index(&s.project.analysis_queue, p)?;
            check_origin(s, &s.project.analysis_queue[i])?;
            Ok(json!({"valid":true}))
        }),
        make("analysis.enqueue", "加入分析队列", enqueue),
        make("analysis.start", "领取分析任务", start),
        make("analysis.probed", "接收音轨探测", probed),
        make("analysis.selectAudio", "确认分析音轨", select),
        make("analysis.cancel", "取消分析任务", cancel),
        make("analysis.fail", "记录分析失败", fail),
        make("analysis.complete", "接收排队分析结果", finish),
        make("analysis.retry", "主动重试分析", retry),
        make("analysis.list", "查看分析队列", |s, _| Ok(json!(s.project.analysis_queue))),
        make("analysis.next", "查看下一个分析任务", |s, _| {
            if s.project.analysis_queue.iter().any(|t| t.status.has_worker()) {
                return Ok(Value::Null);
            }
            Ok(s.project.analysis_queue.iter().find(|t| t.status == Status::Queued).map(value).unwrap_or(Value::Null))
        }),
    ]
}
