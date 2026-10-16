//! One designated editing row, using native video/audio clips and source coordinates.
use filmcraft_project::{EditingTrack, ItemId, Project, Sequence, Track, TrackId, TrackKind};
use filmcraft_time::Tick;
use serde_json::Value;

use crate::commands::{bad, track_p};
use crate::{EngineError, Result, Session};

pub(crate) fn set_target(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("enabled").and_then(Value::as_bool) == Some(false) {
        s.edit_sequence("Clear Editing Track", |q, _, _| {
            q.editing_track = None;
            Ok(())
        })?;
        return Ok(Value::Null);
    }
    let tg = s.targeting();
    let video = track_p(s, p, "track", "sequence.setEditingTrack")?.or(tg.video_dest).ok_or_else(|| bad("sequence.setEditingTrack", "need a video track"))?;
    let audio =
        track_p(s, p, "audioTrack", "sequence.setEditingTrack")?.or(tg.audio_dest).ok_or_else(|| bad("sequence.setEditingTrack", "need an audio track"))?;
    s.edit_sequence("Set Editing Track", |q, _, _| {
        let v = q.video_tracks.iter().find(|t| t.id == video).ok_or_else(|| bad("sequence.setEditingTrack", "track must name a video track"))?;
        let a = q.audio_tracks.iter().find(|t| t.id == audio).ok_or_else(|| bad("sequence.setEditingTrack", "audioTrack must name an audio track"))?;
        let mut end = Tick::ZERO;
        for c in &v.items {
            if c.start != end
                || c.link.is_none()
                || !a.items.iter().any(|x| x.link == c.link && x.start == c.start && x.duration == c.duration && x.item == c.item && x.source_in == c.source_in)
            {
                return Err(bad("sequence.setEditingTrack", "use empty tracks or an existing contiguous, linked A/V edit"));
            }
            end = c.end();
        }
        if a.items.len() != v.items.len() {
            return Err(bad("sequence.setEditingTrack", "audio contains independent clips; choose an empty track"));
        }
        // Adopting an existing edit also adopts its further linked sound outputs. Otherwise
        // clearing and re-enabling the role would leave those sounds behind on the next insert.
        let mut retained = vec![audio];
        retained.extend(
            q.audio_tracks
                .iter()
                .filter(|t| {
                    t.id != audio
                        && !t.items.is_empty()
                        && t.items.iter().all(|a| {
                            a.link.is_some()
                                && v.items.iter().any(|c| {
                                    c.link == a.link && c.start == a.start && c.duration == a.duration && c.item == a.item && c.source_in == a.source_in
                                })
                        })
                })
                .map(|t| t.id),
        );
        q.editing_track = Some(EditingTrack { video, audio: retained });
        Ok(())
    })?;
    Ok(serde_json::to_value(s.active_sequence().and_then(|q| q.editing_track.as_ref())).unwrap_or_default())
}

/// Resolve only the explicitly designated target; all other tracks keep native placement.
/// A pointer over occupied content chooses the closest edit boundary; blank space appends.
pub fn plan(q: &Sequence, video: Option<TrackId>, audio: Option<TrackId>, at: Tick) -> Result<Option<(Tick, TrackId)>> {
    let Some(target) = q.editing_track.as_ref().filter(|t| video == Some(t.video)) else { return Ok(None) };
    let v = q.video_tracks.iter().find(|t| t.id == target.video).ok_or_else(|| EngineError::Other("editing video track was removed".into()))?;
    let a = target
        .audio
        .first()
        .copied()
        .filter(|id| q.audio_tracks.iter().any(|t| t.id == *id))
        .ok_or_else(|| EngineError::Other("editing audio track was removed".into()))?;
    if v.locked || target.audio.iter().any(|id| q.track(*id).is_none_or(|track| track.locked)) {
        return Err(EngineError::Other("an associated editing track is locked or missing".into()));
    }
    if audio.is_some_and(|id| id != a) {
        return Err(EngineError::Other("the editing track keeps its associated audio destination".into()));
    }
    let end = v.end();
    let time = if at >= end {
        end
    } else {
        v.items.iter().flat_map(|i| [i.start, i.end()]).min_by_key(|t| (i128::from(t.0) - i128::from(at.0)).abs()).unwrap_or(Tick::ZERO)
    };
    Ok(Some((time, a)))
}

/// Extra retained streams belong to this editing unit; never commandeer the next native track.
pub(crate) fn ensure_audio(p: &mut Project, sequence: ItemId, count: usize) -> Result<Vec<TrackId>> {
    let q = p.sequence(sequence).ok_or(EngineError::NoSequence)?;
    let mut ids = q.editing_track.as_ref().ok_or_else(|| EngineError::Other("no editing track".into()))?.audio.clone();
    if q.audio_tracks.len().saturating_add(count.saturating_sub(ids.len())) > crate::sequence_tools::MAX_TRACKS {
        return Err(EngineError::Other("too many retained audio tracks".into()));
    }
    while ids.len() < count {
        let id = TrackId(p.alloc_id());
        let q = p.sequence_mut(sequence).ok_or(EngineError::NoSequence)?;
        q.audio_tracks.push(Track::new(id, TrackKind::Audio, format!("Editing Audio {}", ids.len() + 1)));
        ids.push(id);
    }
    p.sequence_mut(sequence).ok_or(EngineError::NoSequence)?.editing_track.as_mut().ok_or_else(|| EngineError::Other("no editing track".into()))?.audio =
        ids.clone();
    Ok(ids)
}
