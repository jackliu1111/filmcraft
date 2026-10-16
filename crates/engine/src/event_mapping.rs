//! Derived sound-event projections. Only recognition results and native clips are persisted.
use crate::commands::{CommandSpec, always};
use crate::{EngineError, Result, Session};
use filmcraft_project::{ClipId, ItemId, TrackId, TrackItem};
use filmcraft_time::Tick;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub item: ItemId,
    pub result_id: String,
    pub category: String,
    pub event_id: String,
    pub event_start: Tick,
    pub event_end: Tick,
    pub source_start: Tick,
    pub source_end: Tick,
    pub sequence_start: Tick,
    pub sequence_end: Tick,
    pub reversed: bool,
    pub held: bool,
    pub clips: Vec<ClipId>,
    pub tracks: Vec<TrackId>,
    pub link: Option<u64>,
}
fn recognized<'a>(s: &'a Session, source: ItemId) -> Option<(ItemId, &'a Value)> {
    let root = s.project.resolve_media(source)?.0;
    s.project
        .recognition
        .get(&root)
        .map(|r| (root, &r["result"]))
        .or_else(|| s.project.recognition.iter().find(|(id, _)| crate::selection_usage::same_source(s, **id, root)).map(|(id, r)| (*id, &r["result"])))
}
/// Project every event in both categories, regardless of which range originally placed the clip.
pub fn for_clip(s: &Session, track: TrackId, clip: &TrackItem) -> Vec<Mapping> {
    let Some((item, result)) = recognized(s, clip.item) else { return Vec::new() };
    let mut mappings = Vec::new();
    for category in ["gunshot", "voice"] {
        let record = &result["categories"][category];
        if record["status"] == "unavailable" {
            continue;
        }
        let events = record["events"].as_array().or_else(|| if category == "gunshot" { result["events"].as_array() } else { None });
        let Some(events) = events else { continue };
        for (index, event) in events.iter().enumerate() {
            let (Some(start), Some(end)) = (event["start_s"].as_f64(), event["end_s"].as_f64()) else { continue };
            if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
                continue;
            }
            // The validated bridge already maps analysis audio offsets into source-video time.
            let event_start = Tick::from_seconds_f64(start);
            let event_end = Tick::from_seconds_f64(end);
            let footprint = crate::selection_usage::source_span(s, clip);
            let source_start = event_start.max(footprint.start);
            let source_end = event_end.min(footprint.end());
            if source_start >= source_end {
                continue;
            }
            if !clip.speed.is_finite() || clip.speed == 0.0 {
                continue;
            }
            let scale = |delta: Tick| Tick((delta.0 as f64 / clip.speed.abs()).round() as i64);
            let (sequence_start, sequence_end) = if clip.frame_hold.is_some() {
                (clip.start, clip.end())
            } else if clip.reverse {
                (clip.start + scale(clip.source_out() - source_end), clip.start + scale(clip.source_out() - source_start))
            } else {
                (clip.start + scale(source_start - clip.source_in), clip.start + scale(source_end - clip.source_in))
            };
            let sequence_start = sequence_start.clamp(clip.start, clip.end());
            let sequence_end = sequence_end.clamp(clip.start, clip.end());
            if sequence_start >= sequence_end {
                continue;
            }
            mappings.push(Mapping {
                item,
                result_id: result["result_id"].as_str().unwrap_or("legacy").into(),
                category: category.into(),
                event_id: event["event_id"].as_str().map(str::to_owned).unwrap_or_else(|| format!("e{}", index + 1)),
                event_start,
                event_end,
                source_start,
                source_end,
                sequence_start,
                sequence_end,
                reversed: clip.reverse,
                held: clip.frame_hold.is_some(),
                clips: vec![clip.id],
                tracks: vec![track],
                link: clip.link,
            });
        }
    }
    mappings
}
pub fn all(s: &Session) -> Vec<Mapping> {
    let mut mappings = Vec::<Mapping>::new();
    if let Some(q) = s.active_sequence() {
        for track in q.all_tracks() {
            for clip in &track.items {
                for mapped in for_clip(s, track.id, clip) {
                    if let Some(same) = mappings.iter_mut().find(|m| {
                        mapped.link.is_some()
                            && m.link == mapped.link
                            && m.item == mapped.item
                            && m.result_id == mapped.result_id
                            && m.category == mapped.category
                            && m.event_id == mapped.event_id
                            && m.source_start == mapped.source_start
                            && m.source_end == mapped.source_end
                            && m.sequence_start == mapped.sequence_start
                            && m.sequence_end == mapped.sequence_end
                            && m.reversed == mapped.reversed
                            && m.held == mapped.held
                    }) {
                        same.clips.extend(mapped.clips);
                        if !same.tracks.contains(&track.id) {
                            same.tracks.push(track.id);
                        }
                    } else {
                        mappings.push(mapped);
                    }
                }
            }
        }
    }
    mappings.sort_by(|a, b| {
        a.sequence_start.cmp(&b.sequence_start).then(a.category.cmp(&b.category)).then(a.event_id.cmp(&b.event_id)).then(a.clips.cmp(&b.clips))
    });
    mappings
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub item: ItemId,
    pub source_time: Tick,
    pub sequence_time: Tick,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_end: Option<Tick>,
    pub clips: Vec<ClipId>,
    pub tracks: Vec<TrackId>,
    pub link: Option<u64>,
}
fn collect_position(positions: &mut Vec<Position>, position: Position) {
    if let Some(same) = positions.iter_mut().find(|p| {
        position.link.is_some()
            && p.link == position.link
            && p.item == position.item
            && p.source_time == position.source_time
            && p.sequence_time == position.sequence_time
            && p.sequence_end == position.sequence_end
    }) {
        same.clips.extend(position.clips);
        same.tracks.extend(position.tracks);
    } else {
        positions.push(position);
    }
}
pub fn positions_at(s: &Session, time: Tick) -> Vec<Position> {
    let mut positions = Vec::new();
    if let Some(q) = s.active_sequence() {
        // Match the native Match Frame preference: upper picture first, then audio.
        for track in q.video_tracks.iter().rev().chain(q.audio_tracks.iter()) {
            for clip in track.items.iter().filter(|clip| clip.range().contains(time)) {
                collect_position(
                    &mut positions,
                    Position {
                        item: s.project.resolve_media(clip.item).map_or(clip.item, |(id, _, _)| id),
                        source_time: clip.source_time_at(time),
                        sequence_time: time,
                        sequence_end: None,
                        clips: vec![clip.id],
                        tracks: vec![track.id],
                        link: clip.link,
                    },
                );
            }
        }
    }
    positions
}
pub fn positions_for_source(s: &Session, item: ItemId, time: Tick) -> Vec<Position> {
    let mut positions = Vec::new();
    if let Some(q) = s.active_sequence() {
        for track in q.all_tracks() {
            for clip in track
                .items
                .iter()
                .filter(|clip| crate::selection_usage::same_source(s, clip.item, item) && crate::selection_usage::source_span(s, clip).contains(time))
            {
                if !clip.speed.is_finite() || clip.speed == 0.0 || clip.duration <= Tick::ZERO {
                    continue;
                }
                let relative = if clip.reverse { clip.source_out() - time - Tick(1) } else { time - clip.source_in };
                let sequence_time = if clip.frame_hold.is_some() {
                    clip.start
                } else {
                    (clip.start + Tick((relative.0 as f64 / clip.speed.abs()).round() as i64)).clamp(clip.start, clip.end() - Tick(1))
                };
                collect_position(
                    &mut positions,
                    Position {
                        item,
                        source_time: clip.source_time_at(sequence_time),
                        sequence_time,
                        sequence_end: clip.frame_hold.map(|_| clip.end()),
                        clips: vec![clip.id],
                        tracks: vec![track.id],
                        link: clip.link,
                    },
                );
            }
        }
    }
    positions.sort_by_key(|p| p.sequence_time);
    positions
}

fn coordinates(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::commands::bad;
    if let Some(source) = p.get("sourceTime") {
        if p.get("time").is_some() {
            return Err(bad("timeline.coordinates", "choose sourceTime or sequence time"));
        }
        let item = p["item"].as_u64().map(ItemId).ok_or_else(|| bad("timeline.coordinates", "source item is required"))?;
        let source = source.as_i64().map(Tick).filter(|t| *t >= Tick::ZERO).ok_or_else(|| bad("timeline.coordinates", "invalid sourceTime"))?;
        let duration = s.project.resolve_media(item).ok_or_else(|| bad("timeline.coordinates", "source is missing"))?.1.duration();
        if source > duration {
            return Err(bad("timeline.coordinates", "sourceTime exceeds media"));
        }
        Ok(json!({"coordinate":"source","item":item,"time":source,"positions":positions_for_source(s,item,source)}))
    } else {
        let time = match p.get("time") {
            Some(v) => v.as_i64().map(Tick).filter(|t| *t >= Tick::ZERO).ok_or_else(|| bad("timeline.coordinates", "invalid sequence time"))?,
            None => s.playhead(),
        };
        Ok(json!({"coordinate":"sequence","time":time,"positions":positions_at(s,time)}))
    }
}
fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    let seq = s.state.active_sequence.ok_or(EngineError::NoSequence)?;
    Ok(json!({"sequence":seq,"mappings":all(s)}))
}
pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "timeline.eventMapping",
            label: "查看声音事件成片映射",
            menu: &[],
            shortcut: None,
            params: "{}",
            enabled: always,
            run: inspect,
            journal: false,
        },
        CommandSpec {
            id: "timeline.coordinates",
            label: "查看原片成片对应位置",
            menu: &[],
            shortcut: None,
            params: r#"{"time":Tick?} | {"item":id,"sourceTime":Tick}"#,
            enabled: always,
            run: coordinates,
            journal: false,
        },
    ]
}
