//! Saved events remain usable evidence even when external prediction files disappear.
use crate::{Result, Session, commands::bad};
use filmcraft_project::{ItemId, MediaRef};
use serde_json::{Value, json};
use std::sync::Arc;

fn item(p: &Value) -> Result<ItemId> {
    p["item"].as_u64().map(ItemId).ok_or_else(|| bad("recognition.recovery", "item is required"))
}
pub(crate) fn inspect(s: &Session, p: &Value) -> Result<Value> {
    let id = item(p)?;
    let record = s.project.recognition.get(&id);
    let source = s.project.item(id).and_then(|i| i.as_media());
    let path = source.and_then(|m| match &m.media {
        MediaRef::File { path } => Some(path.as_str()),
        _ => None,
    });
    let source_error = record.and_then(|r| crate::recognition::check_saved_source(s, id, &r["result"]).err()).map(|e| e.to_string());
    let (status, reason, code) = if source.is_none() || source.is_some_and(|m| m.offline) || path.is_none_or(|p| !std::path::Path::new(p).is_file()) {
        ("source_offline", "原片离线或已移除；保存的事件、人工范围和剪辑仍保留", "source_offline")
    } else if let Some(record) = record {
        if record["result"]["result_id"].as_str().is_none_or(str::is_empty)
            || record["result"]["events"].as_array().is_none()
            || record["audioStream"].as_u64().is_none()
        {
            ("missing", "工程中的分析关联不完整；请主动重试分析，已有工作仍保留", "association_missing")
        } else if let Some(reason) = source_error.as_deref() {
            ("unavailable", reason, "source_changed")
        } else if record["association"]["status"] == "unavailable" {
            (
                "unavailable",
                record["association"]["reason"].as_str().unwrap_or("分析关联不可用；请主动核对或重试"),
                record["association"]["code"].as_str().unwrap_or("association_invalid"),
            )
        } else if s.recognition_jobs.checked.get(&id).is_some_and(|r| Some(r.as_str()) == record["result"]["result_id"].as_str()) {
            ("valid", "已核对完整分析关联", "")
        } else {
            ("unchecked", "已恢复保存的事件与人工工作，等待只读核对分析关联", "")
        }
    } else if s.project.selection_ranges.get(&id).is_some_and(|ranges| ranges.iter().any(|r| r.result_id.is_some()))
        || s.project.assemblies.iter().any(|a| a.source_item == id && a.ranges.iter().any(|r| r.result_id.is_some()))
        || s.project.analysis_queue.iter().any(|t| t.item == id && t.result_id.is_some())
    {
        ("missing", "分析关联缺失；已保存人工范围与剪辑仍保留，请主动重试分析", "association_missing")
    } else {
        ("unanalyzed", "未分析", "")
    };
    Ok(json!({"item":id.0,"status":status,"reason":reason,"code":code,"projectToken":s.recognition_jobs.incarnation,
        "resultId":record.and_then(|r|r["result"]["result_id"].as_str()),"audioStream":record.and_then(|r|r["audioStream"].as_u64()),"path":path}))
}
pub(crate) fn validated(s: &mut Session, p: &Value) -> Result<Value> {
    let id = item(p)?;
    let fail = |message| bad("recognition.validated", message);
    if p["projectToken"].as_u64() != Some(s.recognition_jobs.incarnation) {
        return Err(fail("工程已切换，忽略迟到的关联核对"));
    }
    let record = s.project.recognition.get(&id).ok_or_else(|| fail("保存的分析关联不存在"))?;
    let identifier = record["result"]["result_id"].as_str().ok_or_else(|| fail("保存的分析编号缺失"))?;
    if p["resultId"].as_str() != Some(identifier) {
        return Err(fail("分析已替换，忽略旧关联核对"));
    }
    let association = if let Some(error) = p["error"].as_str().filter(|e| !e.is_empty()) {
        json!({"status":"unavailable","reason":error.chars().take(8192).collect::<String>(),"code":p["code"].as_str().unwrap_or("association_invalid").chars().take(128).collect::<String>()})
    } else {
        let event = &p["validation"];
        if event["status"] != "valid"
            || event["result_id"] != identifier
            || event["prediction_id"] != identifier
            || event["source"] != record["result"]["media"]["source"]
            || event["audio_stream"] != record["audioStream"]
        {
            return Err(fail("关联核对返回了另一份原片、音轨或预测身份"));
        }
        match super::recognition::check_saved(s, id, &record["result"]) {
            Ok(()) => json!({"status":"valid"}),
            Err(error) => json!({"status":"unavailable","code":"source_changed","reason":error.to_string()}),
        }
    };
    let identifier = identifier.to_string();
    if (association["status"] == "valid" && record["association"]["status"] != "unavailable") || record["association"] == association {
        s.recognition_jobs.checked.insert(id, identifier);
        return inspect(s, p);
    }
    let mut updated = record.as_ref().clone();
    updated["association"] = association;
    let updated = Arc::new(updated);
    Arc::make_mut(&mut s.project).recognition.insert(id, updated.clone());
    // Validation is persistence metadata, never a trim or an undo step.
    for (_, snapshot) in s.history.undo.iter_mut().chain(s.history.redo.iter_mut()) {
        if let Some(old) = snapshot.recognition.get(&id) {
            if old["result"]["result_id"] == identifier {
                let mut value = old.as_ref().clone();
                value["association"] = updated["association"].clone();
                Arc::make_mut(snapshot).recognition.insert(id, Arc::new(value));
            }
        }
    }
    s.recognition_jobs.checked.insert(id, identifier);
    s.analysis_metadata_updates = s.analysis_metadata_updates.saturating_add(1);
    s.bump();
    inspect(s, p)
}
