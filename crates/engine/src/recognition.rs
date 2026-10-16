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
    mapping: Option<Value>,
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
    let mapping = if let Some(probe) = p.get("media") {
        Some(map_audio(s, item, probe)?)
    } else {
        if media.info.audio_streams.len() != 1 {
            return Err(invalid("多音轨录像需要先读取并确认音轨映射"));
        }
        None
    };
    if let Some(mapping) = &mapping {
        if !mapping["audioMapping"].as_array().is_some_and(|rows| rows.iter().any(|r| r["containerStream"].as_u64() == Some(audio))) {
            return Err(invalid("所选分析音轨不存在"));
        }
    }
    let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    s.recognition_jobs.pending.insert(token, Origin { item, path, audio, identity: media.identity, size, modified, mapping });
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
    let mapping = map_audio(s, origin.item, media)?;
    if origin.mapping.as_ref().is_some_and(|captured| captured != &mapping) {
        return Err(invalid("分析期间音轨映射已变化"));
    }
    let selected = media.pointer("/selected_audio/stream_index").and_then(Value::as_u64);
    let rows = mapping["audioMapping"].as_array().ok_or_else(|| invalid("音轨映射缺失"))?;
    let row = rows.iter().find(|r| r["containerStream"].as_u64() == Some(origin.audio)).ok_or_else(|| invalid("分析音轨不在原录像中"))?;
    if selected != Some(origin.audio) {
        return Err(invalid("分析结果使用了另一条音轨"));
    }
    let expected_offset = row["audioStartSeconds"].as_f64().ok_or_else(|| invalid("音轨起点无效"))?;
    for (key, expected) in [("audio_to_video_offset_s", expected_offset), ("audio_origin_s", expected_offset), ("video_origin_s", 0.0)] {
        let value = &media["selected_audio"][key];
        if (!value.is_null() || rows.len() > 1) && value.as_f64().is_none_or(|v| !v.is_finite() || (v - expected).abs() > 1e-6) {
            return Err(invalid("分析音轨的原片时间映射不一致"));
        }
    }
    if let Some(offset) = result.pointer("/prediction/audio_to_video_offset_s") {
        if offset.as_f64().is_none_or(|v| !v.is_finite() || (v - expected_offset).abs() > 1e-6) {
            return Err(invalid("完整预测与分析音轨的时间偏移不一致"));
        }
    }
    let duration = media["duration_s"].as_f64().ok_or_else(|| invalid("时长缺失"))?;
    for key in ["events", "candidates", "selection"] {
        if key == "selection" && result.get(key).is_none() {
            continue;
        }
        validate_intervals(&result[key], duration)?;
    }
    if let Some(categories) = result.get("categories") {
        let categories = categories.as_object().filter(|v| v.len() <= 2).ok_or_else(|| invalid("声音类别列表无效"))?;
        for (name, category) in categories {
            if !matches!(name.as_str(), "gunshot" | "voice") || !category.is_object() {
                return Err(invalid("不支持的声音类别"));
            }
            let status = match category.get("status") {
                None => "completed",
                Some(value) => value.as_str().ok_or_else(|| invalid("声音类别状态无效"))?,
            };
            if !matches!(status, "completed" | "unavailable") {
                return Err(invalid("声音类别状态无效"));
            }
            validate_intervals(&category["events"], duration)?;
            if let Some(candidates) = category.get("candidates") {
                validate_intervals(candidates, duration)?;
            }
            if status == "unavailable"
                && (category["reason"].as_str().is_none_or(str::is_empty)
                    || category["events"].as_array().is_none_or(|v| !v.is_empty())
                    || category.get("candidates").and_then(Value::as_array).is_some_and(|v| !v.is_empty()))
            {
                return Err(invalid("不可用类别不得包含候选事件，必须说明原因"));
            }
            if name == "gunshot" && category["events"] != result["events"] {
                return Err(invalid("分类枪声事件与原始枪声结果不一致"));
            }
            if name == "voice"
                && let Some(policy) = category.get("postprocess")
                && policy["version"].as_str() != Some("filmcraft-spoken-voice-v1")
            {
                return Err(invalid("不支持的人声后处理版本"));
            }
        }
    }
    Ok(())
}
/// Native audio streams and ffprobe audio streams are both ordered by container
/// file order. Require equal coverage, increasing container indices and matching
/// per-stream parameters before exposing an explicit container-index/ordinal map.
fn map_audio(s: &Session, id: ItemId, media: &Value) -> Result<Value> {
    let native = s.project.item(id).and_then(|i| i.as_media()).ok_or_else(|| invalid("原录像不存在"))?;
    let MediaRef::File { path } = &native.media else {
        return Err(invalid("需要本地录像"));
    };
    if native.offline || native.proxy.is_some() || native.interpret != Default::default() || native.info.video.is_none() {
        return Err(invalid("音轨选择仅支持在线、无代理且默认解读的原始声画素材"));
    }
    let path = std::fs::canonicalize(path).map_err(|e| invalid(e.to_string()))?;
    let path = path.to_str().ok_or_else(|| invalid("原片路径无效"))?;
    let (size, modified) = file_stamp(path)?;
    if media.pointer("/source/path").and_then(Value::as_str) != Some(path)
        || media.pointer("/source/size_bytes").and_then(Value::as_u64) != Some(size)
        || media.pointer("/source/mtime_ns").and_then(Value::as_u64) != Some(modified)
    {
        return Err(invalid("音轨探测的原录像身份已变化"));
    }
    if media.pointer("/video/start_time_s").and_then(Value::as_f64) != Some(0.0) {
        return Err(invalid("暂不支持非零视频起点的原片时间映射"));
    }
    let duration = media["duration_s"].as_f64().filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| invalid("原片时长无效"))?;
    if (duration - native.info.duration.seconds()).abs() > 0.25 {
        return Err(invalid("编辑器与探测的原片时长不一致"));
    }
    let tracks = media["audio_tracks"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= filmcraft_media::MAX_AUDIO_STREAMS)
        .ok_or_else(|| invalid("原片没有可分析音轨"))?;
    if tracks.len() != native.info.audio_streams.len() {
        return Err(invalid("编辑器与探测的可用音轨数量不一致"));
    }
    let mut mapping = Vec::new();
    let mut previous = None;
    for (ordinal, (track, audio)) in tracks.iter().zip(&native.info.audio_streams).enumerate() {
        let stream = track["stream_index"].as_u64().filter(|v| *v <= u64::from(u32::MAX)).ok_or_else(|| invalid("容器音轨索引无效"))?;
        if previous.is_some_and(|v| stream <= v) {
            return Err(invalid("容器音轨索引重复或顺序无效"));
        }
        previous = Some(stream);
        if audio.sample_rate == 0
            || audio.channels == 0
            || track["sample_rate"].as_u64() != Some(u64::from(audio.sample_rate))
            || track["channels"].as_u64() != Some(u64::from(audio.channels))
        {
            return Err(invalid("编辑器与分析服务的音轨参数不匹配"));
        }
        let start = if track["start_time_s"].is_null() && tracks.len() == 1 {
            0.0
        } else {
            track["start_time_s"].as_f64().filter(|v| v.is_finite() && v.abs() < duration).ok_or_else(|| invalid("音轨起点无效或不可映射"))?
        };
        mapping
            .push(json!({"containerStream":stream,"nativeOrdinal":ordinal,"sampleRate":audio.sample_rate,"channels":audio.channels,"audioStartSeconds":start}));
    }
    Ok(json!({"audioMapping":mapping,"videoOriginSeconds":0.0,"durationSeconds":duration}))
}

fn validate_intervals(values: &Value, duration: f64) -> Result<()> {
    let values = values.as_array().filter(|v| v.len() <= 20_000).ok_or_else(|| invalid("事件列表无效或过大"))?;
    for value in values {
        let start = value["start_s"].as_f64().filter(|v| v.is_finite() && *v >= 0.0);
        let end = value["end_s"].as_f64().filter(|v| v.is_finite() && *v <= duration);
        if !matches!((start,end), (Some(a),Some(b)) if a < b) {
            return Err(invalid("候选或事件范围超出原片"));
        }
    }
    Ok(())
}

fn attach(s: &mut Session, p: &Value) -> Result<Value> {
    let token = token(p)?;
    let origin = s.recognition_jobs.pending.get(&token).ok_or_else(|| invalid("任务已取消或工程已切换"))?.clone();
    let result = p.get("result").ok_or_else(|| invalid("result is required"))?;
    validate(s, &origin, result)?;
    let mapping = map_audio(s, origin.item, &result["media"])?;
    let ordinal = mapping["audioMapping"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["containerStream"].as_u64() == Some(origin.audio)))
        .map(|r| r["nativeOrdinal"].clone())
        .unwrap_or(Value::Null);
    let record = json!({"status":"completed", "audioStream":origin.audio, "analysisAudioOrdinal":ordinal,"audioMapping":mapping["audioMapping"], "sourcePath":origin.path, "result":result});
    s.edit("关联声音识别结果", |pr, _| {
        pr.recognition.insert(origin.item, Arc::new(record.clone()));
        Ok(())
    })?;
    s.recognition_jobs.pending.remove(&token);
    Ok(record)
}
pub fn commands() -> Vec<CommandSpec> {
    let make = |id, label, run, journal| CommandSpec { id, label, menu: &[], shortcut: None, params: "{}", enabled: always, run, journal };
    vec![
        make("recognition.mapAudio", "核对分析音轨映射", |s, p| map_audio(s, item(p)?, &p["media"]), false),
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
