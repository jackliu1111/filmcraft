//! Target editing policies run native operations on an isolated set of owned tracks, then
//! publish one complete sequence snapshot. Source references and native tracks stay untouched.
use crate::{EngineError, Result, Session};
use filmcraft_edit::Edge;
use filmcraft_project::{ClipId, Sequence, TrackId};
use filmcraft_time::Tick;
use serde_json::{Value, json};

fn owned(q: &Sequence, track: TrackId) -> bool {
    q.editing_track.as_ref().is_some_and(|t| t.video == track || t.audio.contains(&track))
}
pub fn is_target(q: &Sequence, clip: ClipId) -> bool {
    q.find_item(clip).is_some_and(|(tid, _)| owned(q, tid))
}

/// Target units always include their linked retained sounds, independently of native Alt /
/// Linked Selection. Non-target selections retain the existing editor behavior.
pub fn selection(q: &Sequence, clips: &[ClipId]) -> Vec<ClipId> {
    let links: Vec<u64> = clips.iter().filter_map(|id| q.find_item(*id).filter(|(tid, _)| owned(q, *tid)).and_then(|(_, c)| c.link)).collect();
    let mut all = clips.to_vec();
    for tr in q.all_tracks().filter(|tr| owned(q, tr.id)) {
        for clip in tr.items.iter().filter(|c| c.link.is_some_and(|link| links.contains(&link))) {
            if !all.contains(&clip.id) {
                all.push(clip.id);
            }
        }
    }
    all
}

fn scoped(q: &Sequence, clips: &[ClipId]) -> Result<Option<(Sequence, Vec<ClipId>)>> {
    if !clips.iter().any(|id| q.find_item(*id).is_some_and(|(tid, _)| owned(q, tid))) {
        return Ok(None);
    }
    if clips.iter().any(|id| q.find_item(*id).is_none_or(|(tid, _)| !owned(q, tid))) {
        return Err(EngineError::Other("edit the associated editing track separately from native tracks".into()));
    }
    let target = q.editing_track.as_ref().ok_or(EngineError::NoSequence)?;
    if !q.video_tracks.iter().any(|track| track.id == target.video)
        || target.audio.is_empty()
        || target.audio.iter().any(|id| !q.audio_tracks.iter().any(|track| track.id == *id))
    {
        return Err(EngineError::Other("the associated editing track has invalid destinations".into()));
    }
    if std::iter::once(target.video).chain(target.audio.iter().copied()).any(|tid| q.track(tid).is_none_or(|tr| tr.locked)) {
        return Err(EngineError::Other("an associated editing track is locked or missing".into()));
    }
    let mut work = q.clone();
    work.video_tracks.retain(|t| t.id == target.video);
    work.audio_tracks.retain(|t| target.audio.contains(&t.id));
    work.caption_tracks.clear();
    Ok(Some((work, selection(q, clips))))
}

fn publish(q: &mut Sequence, work: Sequence) {
    for changed in work.video_tracks.into_iter().chain(work.audio_tracks) {
        if let Some(track) = q.track_mut(changed.id) {
            *track = changed;
        }
    }
}

pub struct Plan {
    pub sequence: Sequence,
    pub units: Vec<ClipId>,
    pub delta: Tick,
    next_id: u64,
}

pub fn trim_plan(s: &Session, clip: ClipId, edge: Edge, delta: Tick) -> Result<Option<Plan>> {
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let Some((mut work, units)) = scoped(q, &[clip])? else { return Ok(None) };
    let durations = |item| crate::media_duration(&s.project, &s.media, item);
    let starts = |item| crate::media_start(&s.project, item);
    let mut next_id = s.project.next_id;
    let rate = q.settings.frame_rate;
    let mut ctx = filmcraft_edit::EditCtx { next_id: &mut next_id, media_duration: &durations, media_start: &starts, min_duration: rate.frame_duration() };
    let delta = filmcraft_edit::ripple_trim_group(&mut work, &units, edge, rate.snap_nearest(delta), &mut ctx)?;
    let mut sequence = q.clone();
    publish(&mut sequence, work);
    Ok(Some(Plan { sequence, units, delta, next_id }))
}

pub(crate) fn trim(s: &mut Session, clip: ClipId, edge: Edge, delta: Tick) -> Result<Option<Value>> {
    let Some(plan) = trim_plan(s, clip, edge, delta)? else { return Ok(None) };
    let delta = plan.delta;
    if delta != Tick::ZERO {
        s.edit_sequence("Trim Editing Unit", move |q, ctx, _| {
            *q = plan.sequence;
            *ctx.next_id = plan.next_id;
            Ok(())
        })?;
    }
    Ok(Some(json!({"delta":delta.0})))
}

pub fn move_plan(s: &Session, moves: &[(ClipId, TrackId, Tick)], copy: bool) -> Result<Option<Plan>> {
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let selected: Vec<_> = moves.iter().map(|m| m.0).collect();
    let Some((mut work, units)) = scoped(q, &selected)? else { return Ok(None) };
    let first = moves.first().ok_or_else(|| EngineError::Other("need moves".into()))?;
    let (_, original) = q.find_item(first.0).ok_or(filmcraft_edit::EditError::NoItem(first.0))?;
    let offset = i128::from(first.2.0) - i128::from(original.start.0);
    for &(clip, dest, time) in moves {
        let (track, item) = q.find_item(clip).ok_or(filmcraft_edit::EditError::NoItem(clip))?;
        if track != dest || i128::from(time.0) - i128::from(item.start.0) != offset {
            return Err(EngineError::Other("move associated editing units together within their editing row".into()));
        }
    }
    let video = &work.video_tracks[0];
    let selected_video: Vec<_> = video.items.iter().filter(|v| units.contains(&v.id)).map(|v| v.id).collect();
    let min_start = video
        .items
        .iter()
        .filter(|v| selected_video.contains(&v.id))
        .map(|v| v.start)
        .min()
        .ok_or_else(|| EngineError::Other("associated picture is missing".into()))?;
    let requested = (i128::from(min_start.0) + offset).max(0);
    if requested > i128::from(Tick::MAX.0) {
        return Err(EngineError::Other("move time is out of range".into()));
    }
    let time = Tick(requested as i64);
    let boundaries: Vec<_> = std::iter::once(Tick::ZERO).chain(video.items.iter().map(|v| v.end())).collect();
    let boundary = boundaries.iter().enumerate().min_by_key(|(_, b)| (i128::from(b.0) - i128::from(time.0)).abs()).map(|(i, _)| i).unwrap_or(0);
    let durations = |item| crate::media_duration(&s.project, &s.media, item);
    let starts = |item| crate::media_start(&s.project, item);
    let mut next_id = s.project.next_id;
    let mut ctx = filmcraft_edit::EditCtx {
        next_id: &mut next_id,
        media_duration: &durations,
        media_start: &starts,
        min_duration: q.settings.frame_rate.frame_duration(),
    };
    let units = if copy {
        let mut at = boundaries[boundary];
        let mut relative = Vec::new();
        for id in &selected_video {
            let v = video.item(*id).ok_or(filmcraft_edit::EditError::NoItem(*id))?;
            for track in work.all_tracks() {
                for item in track.items.iter().filter(|item| item.id == *id || v.link.is_some() && item.link == v.link) {
                    relative.push((item.id, track.id, at));
                }
            }
            at += v.duration;
        }
        filmcraft_edit::copy_items(&mut work, &relative, true, &mut ctx)?
    } else {
        let mut order: Vec<_> = video.items[..boundary].iter().filter(|v| !selected_video.contains(&v.id)).map(|v| v.id).collect();
        order.extend(selected_video.iter().copied());
        order.extend(video.items[boundary..].iter().filter(|v| !selected_video.contains(&v.id)).map(|v| v.id));
        let mut at = Tick::ZERO;
        let mut placements = Vec::new();
        for id in order {
            let v = video.item(id).ok_or(filmcraft_edit::EditError::NoItem(id))?;
            for track in work.all_tracks() {
                for item in track.items.iter().filter(|item| item.id == id || v.link.is_some() && item.link == v.link) {
                    if item.start != v.start || item.duration != v.duration || item.item != v.item || item.source_in != v.source_in {
                        return Err(EngineError::Other("associated audio no longer matches its picture".into()));
                    }
                    placements.push((item.id, track.id, at));
                }
            }
            at += v.duration;
        }
        if placements.len() != work.all_tracks().map(|t| t.items.len()).sum::<usize>() {
            return Err(EngineError::Other("independent audio must be edited on native tracks".into()));
        }
        filmcraft_edit::move_items(&mut work, &placements, false, &mut ctx)?;
        units
    };
    let mut sequence = q.clone();
    publish(&mut sequence, work);
    Ok(Some(Plan { sequence, units, delta: Tick::ZERO, next_id }))
}

pub(crate) fn move_units(s: &mut Session, moves: &[(ClipId, TrackId, Tick)], copy: bool) -> Result<Option<Value>> {
    let Some(plan) = move_plan(s, moves, copy)? else { return Ok(None) };
    let ids: Vec<_> = plan.units.iter().map(|c| c.0).collect();
    if s.active_sequence() != Some(&plan.sequence) {
        s.edit_sequence(if copy { "Copy Editing Units" } else { "Reorder Editing Units" }, move |q, ctx, state| {
            *q = plan.sequence;
            *ctx.next_id = plan.next_id;
            state.selection = plan.units;
            Ok(())
        })?;
    }
    Ok(Some(if copy { json!({"copied":ids,"overwritten":[]}) } else { json!({"moved":ids,"overwritten":[]}) }))
}

pub fn query(s: &Session, p: &Value) -> Result<Value> {
    use crate::commands::{bad, clip_p, track_p};
    let plan = match p.get("action").and_then(Value::as_str) {
        Some("trim") => {
            let clip = clip_p(p, "clip").ok_or_else(|| bad("timeline.planEdit", "need clip"))?;
            let edge = if p["edge"] == "in" { Edge::In } else { Edge::Out };
            let delta = p
                .get("delta")
                .and_then(Value::as_i64)
                .map(Tick)
                .or_else(|| p.get("deltaFrames").and_then(Value::as_i64).map(|f| s.sequence_rate().tick_of(f)))
                .unwrap_or_default();
            trim_plan(s, clip, edge, delta)?
        }
        Some("move") => {
            let mut moves = Vec::new();
            for m in p.get("moves").and_then(Value::as_array).into_iter().flatten() {
                let clip = clip_p(m, "clip").ok_or_else(|| bad("timeline.planEdit", "need clip"))?;
                let track = track_p(s, m, "track", "timeline.planEdit")?.ok_or_else(|| bad("timeline.planEdit", "need track"))?;
                let time = m.get("time").and_then(Value::as_i64).map(Tick).ok_or_else(|| bad("timeline.planEdit", "need time"))?;
                moves.push((clip, track, time));
            }
            move_plan(s, &moves, p.get("copy").and_then(Value::as_bool).unwrap_or(false))?
        }
        _ => return Err(bad("timeline.planEdit", "action must be trim or move")),
    };
    let Some(plan) = plan else { return Ok(json!({"handled":false})) };
    let clips: Vec<_> = plan
        .sequence
        .all_tracks()
        .filter(|t| owned(&plan.sequence, t.id))
        .flat_map(|t| {
            t.items.iter().map(
                move |c| json!({"clip":c.id.0,"track":t.id.0,"start":c.start.0,"duration":c.duration.0,"sourceIn":c.source_in.0,"item":c.item.0,"link":c.link}),
            )
        })
        .collect();
    Ok(json!({"handled":true,"delta":plan.delta.0,"units":plan.units.iter().map(|c|c.0).collect::<Vec<_>>(),"clips":clips}))
}

pub(crate) fn delete(s: &mut Session, clips: &[ClipId]) -> Result<Option<Value>> {
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let Some((mut work, units)) = scoped(q, clips)? else { return Ok(None) };
    filmcraft_edit::ripple_delete_items(&mut work, &units)?;
    s.edit_sequence("Delete Editing Unit", move |q, _, state| {
        publish(q, work);
        state.selection.clear();
        Ok(())
    })?;
    Ok(Some(Value::Null))
}
