//! A source-range snapshot uses the same planner and commit path as ordinary native source drops.
use crate::commands::{CommandSpec, always, bad};
use crate::{EngineError, Result, Session};
use filmcraft_project::{ItemId, MediaRef, TrackId};
use filmcraft_time::{Tick, TimeRange};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub item: ItemId,
    pub source_in: Tick,
    pub duration: Tick,
    pub project_token: u64,
    pub sequence: ItemId,
    pub video_track: TrackId,
    pub audio_track: TrackId,
    #[serde(with = "signature_hex")]
    pub source_signature: u64,
    pub video: bool,
    pub audio: bool,
}
impl Snapshot {
    pub fn range(self) -> TimeRange {
        TimeRange::new(self.source_in, self.duration)
    }
}
mod signature_hex {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{value:016x}"))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
        let text = String::deserialize(d)?;
        u64::from_str_radix(&text, 16).map_err(serde::de::Error::custom)
    }
}
fn invalid(message: impl Into<String>) -> EngineError {
    bad("selection.place", message)
}
fn source_signature(s: &Session, item: ItemId) -> Result<u64> {
    let source = s.project.item(item).and_then(|i| i.as_media()).ok_or_else(|| invalid("原片不存在"))?;
    if source.offline {
        return Err(invalid("原片离线，不能加入剪辑轨"));
    }
    let stamp = match &source.media {
        MediaRef::File { path } => {
            let stat = std::fs::metadata(path).map_err(|e| invalid(format!("无法读取原片：{e}")))?;
            Some((stat.len(), stat.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos())))
        }
        _ => None,
    };
    let mapping =
        serde_json::to_vec(&(&source.media, &source.info, &source.interpret, &source.proxy, source.identity, stamp)).map_err(|e| invalid(e.to_string()))?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    mapping.hash(&mut hash);
    Ok(hash.finish())
}
pub fn capture(s: &Session, item: ItemId, range_id: &str) -> Result<Snapshot> {
    let range = crate::selection::ranges(s, item)?.into_iter().find(|r| r.range_id == range_id).ok_or_else(|| invalid("待剪范围不存在"))?;
    let sequence = s.state.active_sequence.ok_or(EngineError::NoSequence)?;
    let target = s.active_sequence().and_then(|q| q.editing_track.as_ref()).ok_or_else(|| invalid("请先指定声画剪辑轨"))?;
    let audio_track = *target.audio.first().ok_or_else(|| invalid("剪辑轨缺少关联声音"))?;
    let pi = s.project.item(item).ok_or_else(|| invalid("原片不存在"))?;
    let snapshot = Snapshot {
        item,
        source_in: range.start,
        duration: range.duration,
        project_token: s.recognition_jobs.incarnation,
        sequence,
        video_track: target.video,
        audio_track,
        source_signature: source_signature(s, item)?,
        video: pi.has_video(),
        audio: pi.has_audio(),
    };
    validate(s, snapshot)?;
    Ok(snapshot)
}
fn validate(s: &Session, snapshot: Snapshot) -> Result<()> {
    if snapshot.project_token != s.recognition_jobs.incarnation || s.state.active_sequence != Some(snapshot.sequence) {
        return Err(invalid("工程或序列已切换，拖动已取消"));
    }
    let target = s.active_sequence().and_then(|q| q.editing_track.as_ref()).ok_or_else(|| invalid("当前没有声画剪辑轨"))?;
    if target.video != snapshot.video_track || target.audio.first().copied() != Some(snapshot.audio_track) {
        return Err(invalid("目标剪辑轨已变化"));
    }
    let pi = s.project.item(snapshot.item).ok_or_else(|| invalid("原片已移除"))?;
    let end = snapshot.source_in.0.checked_add(snapshot.duration.0).ok_or_else(|| invalid("原片范围溢出"))?;
    if snapshot.source_in < Tick::ZERO || snapshot.duration <= Tick::ZERO || end > pi.duration().0 {
        return Err(invalid("原片范围无效"));
    }
    if snapshot.video != pi.has_video()
        || snapshot.audio != pi.has_audio()
        || (!snapshot.video && !snapshot.audio)
        || source_signature(s, snapshot.item)? != snapshot.source_signature
    {
        return Err(invalid("原片或媒体映射已变化，拖动已取消"));
    }
    Ok(())
}
pub fn parameters(s: &Session, snapshot: Snapshot, p: &Value) -> Result<Value> {
    validate(s, snapshot)?;
    for (field, target) in [("track", snapshot.video_track), ("audioTrack", snapshot.audio_track)] {
        if p.get(field).is_some() && crate::commands::track_p(s, p, field, "selection.place")? != Some(target) {
            return Err(invalid("请放到指定的声画剪辑轨"));
        }
    }
    let append = match p.get("append") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(|| invalid("append must be boolean"))?,
    };
    let time = if append {
        s.active_sequence().and_then(|q| q.track(snapshot.video_track)).map(|t| t.end()).ok_or_else(|| invalid("目标轨不存在"))?
    } else {
        Tick(p["time"].as_i64().filter(|v| *v >= 0).ok_or_else(|| invalid("落点时间无效"))?)
    };
    Ok(
        json!({"item":snapshot.item.0,"sourceIn":snapshot.source_in.0,"duration":snapshot.duration.0,"track":snapshot.video_track.0,"audioTrack":snapshot.audio_track.0,"video":snapshot.video,"audio":snapshot.audio,"time":time.0,"insert":true}),
    )
}
fn snapshot(p: &Value) -> Result<Snapshot> {
    serde_json::from_value(p["snapshot"].clone()).map_err(|e| invalid(format!("拖动快照无效：{e}")))
}
fn place(s: &mut Session, p: &Value) -> Result<Value> {
    let params = parameters(s, snapshot(p)?, p)?;
    crate::commands::place_one(s, &params)
}
fn plan(s: &mut Session, p: &Value) -> Result<Value> {
    let params = parameters(s, snapshot(p)?, p)?;
    crate::commands::preview_placement(s, &params)
}
pub fn commands() -> Vec<CommandSpec> {
    let make = |id, label, run, journal| CommandSpec { id, label, menu: &[], shortcut: None, params: "{}", enabled: always, run, journal };
    vec![
        make(
            "selection.capture",
            "捕获待剪范围",
            |s, p| {
                let item = p["item"].as_u64().map(ItemId).ok_or_else(|| invalid("item is required"))?;
                let id = p["rangeId"].as_str().ok_or_else(|| invalid("rangeId is required"))?;
                serde_json::to_value(capture(s, item, id)?).map_err(|e| invalid(e.to_string()))
            },
            false,
        ),
        make("selection.planPlacement", "预览待剪范围放置", plan, false),
        make("selection.place", "放入当前待剪范围", place, true),
    ]
}
