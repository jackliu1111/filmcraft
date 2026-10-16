//! Persisted analysis requests. Runtime process tokens never survive a reload.
use crate::ItemId;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisTask {
    pub id: u64,
    pub item: ItemId,
    pub attempt: u32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_status: Option<String>,
    pub source_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mtime_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_stream: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
    #[serde(skip)]
    pub token: Option<u64>,
}

pub fn is_empty(tasks: &Arc<Vec<AnalysisTask>>) -> bool {
    tasks.is_empty()
}

pub fn restore<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<AnalysisTask>>, D::Error> {
    let mut tasks = Vec::<AnalysisTask>::deserialize(d)?;
    let mut ids = std::collections::BTreeSet::new();
    if tasks.len() > 4096 || tasks.iter().any(|t| t.id == 0 || !ids.insert(t.id)) {
        return Err(serde::de::Error::custom("invalid analysis queue"));
    }
    for task in &mut tasks {
        if !matches!(task.status.as_str(), "completed" | "cancelled" | "failed" | "unavailable" | "interrupted") {
            task.previous_status = Some(std::mem::replace(&mut task.status, "interrupted".into()));
            task.error = Some("上次未完成的分析已中断；请主动重试，不会自动恢复模型任务。".into());
        }
        task.token = None;
    }
    Ok(Arc::new(tasks))
}
