//! Persisted analysis requests. Runtime process tokens never survive a reload.
use crate::ItemId;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;

/// Queue state shared by dispatch, late-result protection and project recovery.
/// Its wire representation remains the existing string, including legacy unknown values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisStatus {
    Queued,
    Probing,
    WaitingAudio,
    Running,
    Completed,
    Cancelled,
    Failed,
    Unavailable,
    Interrupted,
    Unknown(String),
}
impl AnalysisStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Queued => "queued",
            Self::Probing => "probing",
            Self::WaitingAudio => "waiting_audio",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
            Self::Interrupted => "interrupted",
            Self::Unknown(value) => value,
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed | Self::Unavailable | Self::Interrupted)
    }
    pub fn has_worker(&self) -> bool {
        matches!(self, Self::Probing | Self::Running)
    }
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Queued | Self::WaitingAudio | Self::Probing | Self::Running)
    }
    pub fn waits_for_worker(&self) -> bool {
        matches!(self, Self::Queued | Self::Probing | Self::Running)
    }
    pub fn can_retry(&self) -> bool {
        self.is_terminal()
    }
    pub fn can_cancel(&self) -> bool {
        !matches!(self, Self::Completed | Self::Cancelled)
    }
}
impl From<&str> for AnalysisStatus {
    fn from(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "probing" => Self::Probing,
            "waiting_audio" => Self::WaitingAudio,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            "unavailable" => Self::Unavailable,
            "interrupted" => Self::Interrupted,
            other => Self::Unknown(other.into()),
        }
    }
}
impl From<String> for AnalysisStatus {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}
impl Serialize for AnalysisStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for AnalysisStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from(String::deserialize(d)?))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisTask {
    pub id: u64,
    pub item: ItemId,
    pub attempt: u32,
    pub status: AnalysisStatus,
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
        if !task.status.is_terminal() {
            task.previous_status = Some(std::mem::replace(&mut task.status, AnalysisStatus::Interrupted).as_str().to_owned());
            task.error = Some("上次未完成的分析已中断；请主动重试，不会自动恢复模型任务。".into());
        }
        task.token = None;
    }
    Ok(Arc::new(tasks))
}
