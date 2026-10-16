//! Project-local clip selection ranges; model event times remain immutable evidence.
use crate::{ClipId, ItemId};
use filmcraft_time::{Tick, TimeRange};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionRange {
    pub range_id: String,
    pub category: String,
    #[serde(default)]
    pub event_ids: Vec<String>,
    #[serde(default)]
    pub result_id: Option<String>,
    pub start: Tick,
    pub duration: Tick,
    #[serde(default)]
    pub revision: u64,
    /// A user-drawn range has no invented model event.
    #[serde(default)]
    pub manual: bool,
    /// Boundary edits survive refreshed analysis.
    #[serde(default)]
    pub modified: bool,
}
impl SelectionRange {
    pub fn range(&self) -> TimeRange {
        TimeRange::new(self.start, self.duration)
    }
}

/// Immutable provenance of one explicit assembly. The files are independent native projects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assembly {
    pub id: String,
    pub source_item: ItemId,
    pub sequence: ItemId,
    pub categories: Vec<String>,
    pub ranges: Vec<SelectionRange>,
    pub segments: Vec<AssemblySegment>,
    pub original_project: String,
    pub edit_project: String,
    pub preserved_project: String,
    pub parent_project: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssemblySegment {
    pub source_in: Tick,
    pub duration: Tick,
    pub start: Tick,
    pub range_ids: Vec<String>,
    pub clips: Vec<ClipId>,
}
