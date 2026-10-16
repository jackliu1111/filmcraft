//! Completed analysis attaches to the captured recording. It never edits a sequence.
use crate::commands::{CommandSpec, always, bad};
use crate::{EngineError, Result, Session};
use filmcraft_project::{ItemId, MediaIdentity, MediaRef};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub struct Origin {
    item: ItemId,
    path: String,
    audio: u64,
    identity: Option<MediaIdentity>,
    size: u64,
    modified: u64,
}
pub struct Jobs {
    pub pending: BTreeMap<u64, Origin>,
    pub imports: std::collections::VecDeque<ItemId>,
    pub incarnation: u64,
}
impl Default for Jobs {
    fn default() -> Self {
        Self { pending: BTreeMap::new(), imports: Default::default(), incarnation: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }
    }
}
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn invalid(message: impl Into<String>) -> EngineError {
    bad("recognition", message)
}
fn item(p: &Value) -> Result<ItemId> {
    p["item"].as_u64().map(ItemId).ok_or_else(|| invalid("item is required"))
}
fn token(p: &Value) -> Result<u64> {
    p["token"].as_u64().ok_or_else(|| invalid("token is required"))
}
fn file_stamp(path: &str) -> Result<(u64, u64)> {
    let st = std::fs::metadata(path).map_err(|e| invalid(format!("无法读取原录像：{e}")))?;
    let time = st
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|t| u64::try_from(t.as_nanos()).ok())
        .ok_or_else(|| invalid("无法核对原录像修改时间"))?;
    Ok((st.len(), time))
}
fn begin(s: &mut Session, p: &Value) -> Result<Value> {
    let item = item(p)?;
    let audio = p["audioStream"].as_u64().ok_or_else(|| invalid("audioStream is required"))?;
    let media = s.project.item(item).and_then(|i| i.as_media()).ok_or_else(|| invalid("原录像不存在"))?;
    let MediaRef::File { path } = &media.media else { return Err(invalid("请选择本地录像")) };
    if media.offline || media.proxy.is_some() || media.interpret != Default::default() || media.info.video.is_none() || media.info.audio_streams.is_empty() {
        return Err(invalid("当前原录像不可分析：需要默认解读的在线声画素材"));
    }
    let path = std::fs::canonicalize(path).map_err(|e| invalid(e.to_string()))?.to_str().ok_or_else(|| invalid("原片路径无效"))?.to_string();
    let (size, modified) = file_stamp(&path)?;
    let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    s.recognition_jobs.pending.insert(token, Origin { item, path, audio, identity: media.identity, size, modified });
    Ok(json!({"token":token,"item":item.0,"audioStream":audio}))
}
fn validate(s: &Session, origin: &Origin, result: &Value) -> Result<()> {
    if result["schema_version"].as_u64() != Some(1) || result["time_unit"].as_str() != Some("seconds") || result["result_id"].as_str().is_none_or(str::is_empty)
    {
        return Err(invalid("不支持或不完整的分析结果"));
    }
    let item = s.project.item(origin.item).and_then(|i| i.as_media()).ok_or_else(|| invalid("原录像已移除"))?;
    let MediaRef::File { path } = &item.media else { return Err(invalid("原录像已变化")) };
    let canonical = std::fs::canonicalize(path).map_err(|e| invalid(e.to_string()))?;
    if canonical.to_str() != Some(&origin.path)
        || item.identity != origin.identity
        || item.offline
        || item.proxy.is_some()
        || item.interpret != Default::default()
        || file_stamp(&origin.path)? != (origin.size, origin.modified)
    {
        return Err(invalid("分析期间原录像或媒体设置已变化"));
    }
    let media = &result["media"];
    if media.pointer("/source/path").and_then(Value::as_str) != Some(&origin.path)
        || media.pointer("/source/size_bytes").and_then(Value::as_u64) != Some(origin.size)
        || media.pointer("/source/mtime_ns").and_then(Value::as_u64) != Some(origin.modified)
    {
        return Err(invalid("分析结果的原录像身份不匹配"));
    }
    let tracks = media["audio_tracks"].as_array().ok_or_else(|| invalid("结果缺少音轨"))?;
    if tracks.len() != item.info.audio_streams.len()
        || tracks.len() != 1
        || tracks[0]["stream_index"].as_u64() != Some(origin.audio)
        || media.pointer("/selected_audio/stream_index").and_then(Value::as_u64) != Some(origin.audio)
    {
        return Err(invalid("分析结果的音轨不匹配；多音轨需要显式映射"));
    }
    let audio = &item.info.audio_streams[0];
    if tracks[0]["sample_rate"].as_u64() != Some(u64::from(audio.sample_rate)) || tracks[0]["channels"].as_u64() != Some(u64::from(audio.channels)) {
        return Err(invalid("编辑器与分析服务的音轨参数不匹配"));
    }
    if media.pointer("/video/start_time_s").and_then(Value::as_f64) != Some(0.0) {
        return Err(invalid("非零视频起点尚未支持"));
    }
    let duration = media["duration_s"].as_f64().filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| invalid("原片时长无效"))?;
    if (duration - item.info.duration.seconds()).abs() > 0.25 {
        return Err(invalid("分析结果与原片时长不一致"));
    }
    for key in ["events", "candidates", "selection"] {
        let Some(values) = result.get(key) else {
            if key == "selection" {
                continue;
            }
            return Err(invalid("缺少候选或事件"));
        };
        let values = values.as_array().filter(|v| v.len() <= 20_000).ok_or_else(|| invalid("事件列表无效或过大"))?;
        for v in values {
            let start = v["start_s"].as_f64().filter(|v| v.is_finite() && *v >= 0.0);
            let end = v["end_s"].as_f64().filter(|v| v.is_finite() && *v <= duration);
            if !matches!((start,end), (Some(a),Some(b)) if a < b) {
                return Err(invalid("候选或事件范围超出原片"));
            }
        }
    }
    Ok(())
}
fn attach(s: &mut Session, p: &Value) -> Result<Value> {
    let token = token(p)?;
    let origin = s.recognition_jobs.pending.get(&token).ok_or_else(|| invalid("任务已取消或工程已切换"))?.clone();
    let result = p.get("result").ok_or_else(|| invalid("result is required"))?;
    validate(s, &origin, result)?;
    let record = json!({"status":"completed", "audioStream":origin.audio, "sourcePath":origin.path, "result":result});
    s.edit("关联声音识别结果", |pr, _| {
        crate::selection::initialize(pr, origin.item, result)?;
        pr.recognition.insert(origin.item, Arc::new(record.clone()));
        Ok(())
    })?;
    s.recognition_jobs.pending.remove(&token);
    Ok(record)
}
pub fn commands() -> Vec<CommandSpec> {
    let make = |id, label, run, journal| CommandSpec { id, label, menu: &[], shortcut: None, params: "{}", enabled: always, run, journal };
    vec![
        make("recognition.begin", "开始声音识别", begin, false),
        make("recognition.attach", "关联声音识别", attach, true),
        make(
            "recognition.cancel",
            "取消声音识别",
            |s, p| {
                s.recognition_jobs.pending.remove(&token(p)?);
                Ok(Value::Null)
            },
            false,
        ),
        make("recognition.inspect", "查看声音识别", |s, p| Ok(s.project.recognition.get(&item(p)?).map(|v| v.as_ref().clone()).unwrap_or(Value::Null)), false),
    ]
}
