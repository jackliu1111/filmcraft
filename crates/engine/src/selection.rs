//! Independent, frame-aligned source selections shared by UI, automation and persistence.
use crate::commands::{CommandSpec, always, bad};
use crate::{EngineError, Result, Session};
use filmcraft_project::{ItemId, Project, SelectionRange};
use filmcraft_time::Tick;
use serde_json::{Value, json};

fn invalid(message: impl Into<String>) -> EngineError {
    bad("selection", message)
}
fn item(p: &Value) -> Result<ItemId> {
    p["item"].as_u64().map(ItemId).ok_or_else(|| invalid("item is required"))
}
fn defaults(project: &Project, item: ItemId, result: &Value) -> Result<Vec<SelectionRange>> {
    let media = project.item(item).and_then(|i| i.as_media()).ok_or_else(|| invalid("原录像不存在"))?;
    let rate = media.frame_rate();
    if rate.num <= 0 || rate.den <= 0 {
        return Err(invalid("原片帧率无效"));
    }
    let limit = rate.snap(media.duration());
    let result_id = result["result_id"].as_str().unwrap_or("legacy");
    let mut ranges = Vec::new();
    for (category, padding) in [("gunshot", 5.0), ("voice", 1.0)] {
        let events = result
            .pointer(&format!("/categories/{category}/events"))
            .and_then(Value::as_array)
            .or_else(|| (category == "gunshot").then(|| result["events"].as_array()).flatten());
        let Some(events) = events else { continue };
        for (index, event) in events.iter().enumerate() {
            let (Some(start), Some(end)) = (event["start_s"].as_f64(), event["end_s"].as_f64()) else { return Err(invalid("识别事件缺少时间")) };
            if !start.is_finite() || !end.is_finite() || start < 0.0 || start >= end || end > media.duration().seconds() + 0.25 {
                return Err(invalid("识别事件范围无效"));
            }
            let start = rate.snap_nearest(Tick::from_seconds_f64((start - padding).max(0.0))).min(limit);
            let end = rate.snap_nearest(Tick::from_seconds_f64((end + padding).min(limit.seconds()))).min(limit);
            if end <= start {
                continue;
            }
            let event_id = event["event_id"].as_str().map(str::to_string).unwrap_or_else(|| format!("e{}", index + 1));
            let range_id = format!("{result_id}:{category}:{event_id}");
            if ranges.iter().any(|r: &SelectionRange| r.range_id == range_id) {
                return Err(invalid("重复的识别事件标识"));
            }
            ranges.push(SelectionRange {
                range_id,
                category: category.into(),
                event_ids: vec![event_id],
                result_id: Some(result_id.into()),
                start,
                duration: end - start,
                revision: 0,
                manual: false,
                modified: false,
            });
        }
    }
    Ok(ranges)
}
/// Called inside the recognition-attachment transaction. Refresh defaults and preserve user work.
pub fn initialize(project: &mut Project, item: ItemId, result: &Value) -> Result<()> {
    let fresh = defaults(project, item, result)?;
    let ranges = project.selection_ranges.entry(item).or_default();
    ranges.retain(|r| r.manual || r.modified);
    for r in fresh {
        if !ranges.iter().any(|old| old.range_id == r.range_id) {
            ranges.push(r)
        }
    }
    ranges.sort_by(|a, b| a.start.cmp(&b.start).then(a.range_id.cmp(&b.range_id)));
    Ok(())
}
/// Older projects derive untouched defaults until their first range edit.
pub fn ranges(s: &Session, item: ItemId) -> Result<Vec<SelectionRange>> {
    if s.project.item(item).and_then(|i| i.as_media()).is_none() {
        return Err(invalid("原录像不存在"));
    }
    let mut ranges = if let Some(ranges) = s.project.selection_ranges.get(&item) {
        ranges.clone()
    } else if let Some(record) = s.project.recognition.get(&item) {
        defaults(&s.project, item, &record["result"])?
    } else {
        Vec::new()
    };
    ranges.sort_by(|a, b| a.start.cmp(&b.start).then(a.range_id.cmp(&b.range_id)));
    Ok(ranges)
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let item = item(p)?;
    let ranges = ranges(s, item)?
        .iter()
        .map(|range| {
            let mut value = serde_json::to_value(range).unwrap_or(Value::Null);
            value["startSeconds"] = json!(range.start.seconds());
            value["endSeconds"] = json!((range.start + range.duration).seconds());
            value
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"item":item.0,"projectToken":s.recognition_jobs.incarnation,"ranges":ranges,"events":s.project.recognition.get(&item).map(|r|&r["result"]["events"]),"categories":s.project.recognition.get(&item).map(|r|&r["result"]["categories"])}),
    )
}
fn check_project(s: &Session, p: &Value) -> Result<()> {
    if let Some(token) = p.get("projectToken") {
        if token.as_u64() != Some(s.recognition_jobs.incarnation) {
            return Err(invalid("工程已切换，待剪范围预览已过期"));
        }
    }
    Ok(())
}
fn normalized(s: &Session, item: ItemId, p: &Value) -> Result<filmcraft_time::TimeRange> {
    let source = s.project.item(item).and_then(|i| i.as_media()).ok_or_else(|| invalid("原录像不存在"))?;
    let start = p["start"].as_i64().ok_or_else(|| invalid("start must be Tick"))?;
    let duration = p["duration"].as_i64().ok_or_else(|| invalid("duration must be Tick"))?;
    let end = start.checked_add(duration).ok_or_else(|| invalid("待剪范围溢出"))?;
    if start < 0 || duration <= 0 || end > source.duration().0 {
        return Err(invalid("待剪范围反向、为空或超出原片"));
    }
    let rate = source.frame_rate();
    if rate.num <= 0 || rate.den <= 0 || rate.frame_duration() <= Tick::ZERO {
        return Err(invalid("原片帧率无效"));
    }
    let limit = rate.snap(source.duration());
    let start = rate.snap_nearest(Tick(start)).min(limit);
    let end = rate.snap_nearest(Tick(end)).min(limit);
    if end <= start {
        return Err(invalid("待剪范围不足一帧"));
    }
    Ok(filmcraft_time::TimeRange::from_bounds(start, end))
}
fn set_range(s: &mut Session, p: &Value) -> Result<Value> {
    check_project(s, p)?;
    let item = item(p)?;
    let id = p["rangeId"].as_str().ok_or_else(|| invalid("rangeId is required"))?;
    let range = normalized(s, item, p)?;
    let mut ranges = ranges(s, item)?;
    let current = ranges.iter_mut().find(|r| r.range_id == id).ok_or_else(|| invalid("待剪范围已不存在"))?;
    if let Some(expected) = p.get("expectedRevision") {
        if expected.as_u64() != Some(current.revision) {
            return Err(invalid("待剪范围已变化，请重新调整"));
        }
    }
    if current.start == range.start && current.duration == range.duration {
        return serde_json::to_value(current).map_err(|e| invalid(e.to_string()));
    }
    current.start = range.start;
    current.duration = range.duration;
    current.modified = true;
    current.revision = current.revision.checked_add(1).ok_or_else(|| invalid("待剪范围版本耗尽"))?;
    let changed = serde_json::to_value(current).map_err(|e| invalid(e.to_string()))?;
    s.edit("调整待剪范围", |project, _| {
        project.selection_ranges.insert(item, ranges);
        Ok(())
    })?;
    Ok(changed)
}

fn add_range(s: &mut Session, p: &Value) -> Result<Value> {
    check_project(s, p)?;
    let item = item(p)?;
    let range = normalized(s, item, p)?;
    let category = p["category"].as_str().unwrap_or("gunshot");
    if !matches!(category, "gunshot" | "voice") {
        return Err(invalid("不支持的待剪类别"));
    }
    let mut ranges = ranges(s, item)?;
    if ranges.len() >= 20_000 {
        return Err(invalid("待剪范围数量过多"));
    }
    let added = s.edit("手动划选待剪范围", |project, _| {
        if project.next_id == u64::MAX {
            return Err(invalid("工程标识耗尽"));
        }
        let id = project.alloc_id();
        let added = SelectionRange {
            range_id: format!("manual-{id}"),
            category: category.into(),
            event_ids: Vec::new(),
            result_id: None,
            start: range.start,
            duration: range.duration,
            revision: 0,
            manual: true,
            modified: false,
        };
        ranges.push(added.clone());
        ranges.sort_by(|a, b| a.start.cmp(&b.start).then(a.range_id.cmp(&b.range_id)));
        project.selection_ranges.insert(item, ranges);
        Ok(added)
    })?;
    serde_json::to_value(added).map_err(|e| invalid(e.to_string()))
}

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "selection.addRange",
            label: "手动补充待剪范围",
            menu: &[],
            shortcut: None,
            params: r#"{"item":id,"start":Tick,"duration":Tick,"category":str?,"projectToken":u64?}"#,
            enabled: always,
            run: add_range,
            journal: true,
        },
        CommandSpec {
            id: "selection.setRange",
            label: "调整待剪范围",
            menu: &[],
            shortcut: None,
            params: r#"{"item":id,"rangeId":str,"start":Tick,"duration":Tick,"expectedRevision":u64?,"projectToken":u64?}"#,
            enabled: always,
            run: set_range,
            journal: true,
        },
        CommandSpec {
            id: "selection.inspect",
            label: "查看待剪范围",
            menu: &[],
            shortcut: None,
            params: r#"{"item":id}"#,
            enabled: always,
            run: inspect,
            journal: false,
        },
    ]
}
