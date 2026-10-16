//! Usage is derived from the current sequence's native source intervals, never a saved boolean.
use crate::commands::{CommandSpec, always, bad};
use crate::{Result, Session};
use filmcraft_project::{ItemId, MediaRef};
use filmcraft_time::{Tick, TimeRange};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub status: &'static str,
    pub uses: usize,
    pub covered: Tick,
    pub total: Tick,
}
fn same_source(s: &Session, left: ItemId, right: ItemId) -> bool {
    let (Some((li, lm, _)), Some((ri, rm, _))) = (s.project.resolve_media(left), s.project.resolve_media(right)) else { return false };
    if li == ri {
        return true;
    }
    match (&lm.media, &rm.media) {
        (MediaRef::File { path: l }, MediaRef::File { path: r }) => {
            lm.identity == rm.identity
                && lm.interpret == rm.interpret
                && std::fs::canonicalize(l).ok().zip(std::fs::canonicalize(r).ok()).is_some_and(|(l, r)| l == r)
        }
        _ => false,
    }
}
pub fn usage(s: &Session, item: ItemId, range: TimeRange) -> Usage {
    let mut units = BTreeMap::<(bool, u64), Vec<TimeRange>>::new();
    if let Some(q) = s.active_sequence() {
        for clip in q.all_tracks().flat_map(|t| t.items.iter()).filter(|c| same_source(s, c.item, item)) {
            let span = if let Some(hold) = clip.frame_hold {
                TimeRange::new(hold, s.project.item(clip.item).map(|i| i.frame_rate().frame_duration()).unwrap_or(Tick(1)))
            } else {
                TimeRange::from_bounds(clip.source_in, clip.source_out())
            };
            let (start, end) = (span.start.max(range.start), span.end().min(range.end()));
            if start < end {
                units.entry(clip.link.map(|id| (true, id)).unwrap_or((false, clip.id.0))).or_default().push(TimeRange::from_bounds(start, end));
            }
        }
    }
    let uses = units.len();
    let mut intervals = units.into_values().flatten().collect::<Vec<_>>();
    intervals.sort_by_key(|r| r.start);
    let mut union = Vec::<TimeRange>::new();
    for span in intervals {
        if let Some(last) = union.last_mut().filter(|last| span.start <= last.end()) {
            last.duration = last.end().max(span.end()) - last.start;
        } else {
            union.push(span)
        }
    }
    let covered = union.iter().fold(Tick::ZERO, |sum, span| sum + span.duration);
    Usage {
        status: if covered <= Tick::ZERO {
            "unused"
        } else if covered >= range.duration {
            "used"
        } else {
            "partial"
        },
        uses,
        covered,
        total: range.duration,
    }
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let item = p["item"].as_u64().map(ItemId).ok_or_else(|| bad("selection.usage", "item is required"))?;
    let range = if let Some(id) = p["rangeId"].as_str() {
        crate::selection::ranges(s, item)?.into_iter().find(|r| r.range_id == id).ok_or_else(|| bad("selection.usage", "rangeId is missing"))?.range()
    } else {
        let start = p["start"].as_i64().filter(|v| *v >= 0).ok_or_else(|| bad("selection.usage", "invalid start"))?;
        let duration = p["duration"].as_i64().filter(|v| *v > 0).ok_or_else(|| bad("selection.usage", "invalid duration"))?;
        let end = start.checked_add(duration).ok_or_else(|| bad("selection.usage", "range overflow"))?;
        let limit = s.project.item(item).ok_or_else(|| bad("selection.usage", "source is missing"))?.duration();
        if end > limit.0 {
            return Err(bad("selection.usage", "range exceeds source"));
        }
        TimeRange::new(Tick(start), Tick(duration))
    };
    Ok(json!(usage(s, item, range)))
}
pub fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "selection.usage",
        label: "查看待剪范围使用情况",
        menu: &[],
        shortcut: None,
        params: r#"{"item":id,"rangeId":str | "start":Tick,"duration":Tick}"#,
        enabled: always,
        run: inspect,
        journal: false,
    }]
}
