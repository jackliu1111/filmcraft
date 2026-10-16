//! Explicit assembly snapshots current source ranges; it never starts analysis or edits the caller in place.
use crate::commands::{CommandSpec, always, bad};
use crate::{Result, Session};
use filmcraft_project::selection::{Assembly, AssemblySegment};
use filmcraft_project::{ItemId, SelectionRange};
use filmcraft_time::Tick;
use serde_json::{Value, json};
use std::path::PathBuf;

fn invalid(message: impl Into<String>) -> crate::EngineError {
    bad("selection.assemble", message)
}

struct Segment {
    source_in: Tick,
    duration: Tick,
    ranges: Vec<String>,
}

fn plan(s: &Session, item: ItemId, p: &Value) -> Result<(Vec<String>, Vec<SelectionRange>, Vec<Segment>)> {
    let media = s.project.item(item).and_then(|i| i.as_media()).ok_or_else(|| invalid("请选择原录像"))?;
    if media.offline || media.proxy.is_some() || media.interpret != Default::default() || media.info.video.is_none() || media.info.audio_streams.is_empty() {
        return Err(invalid("组稿需要在线、无代理且默认解读的原始声画素材"));
    }
    if media.info.audio_streams.iter().any(|a| a.sample_rate == 0 || a.channels == 0) {
        return Err(invalid("原片音轨参数无效"));
    }
    let categories: Vec<String> = match p.get("categories") {
        None => vec!["gunshot".into()],
        Some(v) => v
            .as_array()
            .and_then(|a| a.iter().map(|v| v.as_str().filter(|v| matches!(*v, "gunshot" | "voice")).map(str::to_string)).collect())
            .ok_or_else(|| invalid("组稿类别仅支持 gunshot、voice"))?,
    };
    let rate = media.frame_rate();
    if rate.num <= 0 || rate.den <= 0 || rate.frame_duration() <= Tick::ZERO {
        return Err(invalid("原片帧率无效"));
    }
    let limit = rate.snap(media.duration());
    let mut selected = crate::selection::ranges(s, item)?.into_iter().filter(|r| categories.contains(&r.category)).collect::<Vec<_>>();
    if selected.len() > 20_000 {
        return Err(invalid("组稿范围数量过多"));
    }
    selected.sort_by(|a, b| a.start.cmp(&b.start).then(a.range_id.cmp(&b.range_id)));
    let mut segments: Vec<Segment> = Vec::new();
    for range in &selected {
        let end = range.start.0.checked_add(range.duration.0).ok_or_else(|| invalid("组稿范围溢出"))?;
        if range.start < Tick::ZERO || range.duration <= Tick::ZERO || end > media.duration().0 {
            return Err(invalid("组稿范围为空、反向或超出原片"));
        }
        let start = rate.snap_nearest(range.start).min(limit);
        let end = rate.snap_nearest(Tick(end)).min(limit);
        if end <= start {
            return Err(invalid("组稿范围不足一帧"));
        }
        if let Some(last) = segments.last_mut().filter(|r| start <= r.source_in + r.duration) {
            last.duration = (last.source_in + last.duration).max(end) - last.source_in;
            last.ranges.push(range.range_id.clone());
        } else {
            segments.push(Segment { source_in: start, duration: end - start, ranges: vec![range.range_id.clone()] });
        }
    }
    Ok((categories, selected, segments))
}

struct OutputFolder {
    path: PathBuf,
    committed: bool,
}
impl Drop for OutputFolder {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
fn folder(s: &Session, p: &Value) -> Result<OutputFolder> {
    let root = match p.get("directory") {
        Some(v) => PathBuf::from(v.as_str().ok_or_else(|| invalid("directory 必须是绝对路径"))?),
        None => std::env::var_os("FILMCRAFT_APEX_PROJECTS_DIR")
            .map(PathBuf::from)
            .or_else(|| s.prefs_path.as_ref().and_then(|p| p.parent()).map(|p| p.join("apex-ai-projects")))
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../outputs/apex-ai/projects")),
    };
    if !root.is_absolute() {
        return Err(invalid("组稿保存目录必须是绝对路径"));
    }
    std::fs::create_dir_all(&root).map_err(|e| invalid(format!("无法创建组稿目录：{e}")))?;
    let root = std::fs::canonicalize(root).map_err(|e| invalid(e.to_string()))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| invalid(e.to_string()))?.as_nanos();
    for suffix in 0..16 {
        let path = root.join(format!("draft-{now}-{}-{suffix}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(OutputFolder { path, committed: false }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(invalid(format!("无法创建独立组稿目录：{e}"))),
        }
    }
    Err(invalid("无法分配独立组稿目录"))
}
fn assemble(s: &mut Session, p: &Value) -> Result<Value> {
    if cfg!(target_arch = "wasm32") {
        return Err(invalid("当前组稿保存仅支持桌面版"));
    }
    if p.get("projectToken").is_some_and(|token| token.as_u64() != Some(s.recognition_jobs.incarnation)) {
        return Err(invalid("工程已切换，组稿已取消"));
    }
    let item = match p.get("item") {
        Some(v) => v.as_u64().map(ItemId),
        None => s.state.source_item,
    }
    .ok_or_else(|| invalid("item is required"))?;
    let (categories, ranges, segments) = plan(s, item, p)?;
    if segments.is_empty() {
        return Ok(json!({"status":"noRanges","message":"当前类别没有可组稿范围，已有剪辑未改变。"}));
    }
    let signature = crate::selection_placement::source_signature(s, item)?;
    if let Some(record) = s.project.recognition.get(&item).cloned() {
        // Reuse the checked container/native stream map and original-file stamp. Saved analysis
        // may remain visible after its file changes, but it must not silently assemble new media.
        s.execute("recognition.mapAudio", json!({"item":item.0,"media":record["result"]["media"]}))?;
    }
    let media = s.project.item(item).and_then(|i| i.as_media()).ok_or_else(|| invalid("原录像不存在"))?;
    let settings = crate::commands::default_seq_settings_for(media);
    settings.validate().map_err(invalid)?;
    // Build against an isolated snapshot: child commands, undo and auto-save cannot touch the caller.
    let mut draft = Session::new(s.services.clone());
    draft.project = s.project.clone();
    draft.media = s.media.clone();
    draft.state = s.state.clone();
    draft.prefs = s.prefs.clone();
    let sequence = draft.edit("按当前范围组稿", |project, state| {
        let id = project.new_sequence("组稿编辑副本", settings, 1, 1, None);
        state.active_sequence = Some(id);
        state.open_sequences = vec![id];
        state.selection.clear();
        Ok(id)
    })?;
    draft.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"}))?;
    draft.execute("source.open", json!({"item":item.0}))?;
    let mut cursor = Tick::ZERO;
    let mut mapping = Vec::new();
    for segment in &segments {
        let placed = draft.execute(
            "timeline.place",
            json!({"item":item.0,"track":"V1","audioTrack":"A1","time":cursor.0,
            "sourceIn":segment.source_in.0,"duration":segment.duration.0,"video":true,"audio":true,"insert":true}),
        )?;
        let clips = serde_json::from_value(placed["clips"].clone()).map_err(|e| invalid(format!("原生放置没有返回片段：{e}")))?;
        mapping.push(AssemblySegment { source_in: segment.source_in, duration: segment.duration, start: cursor, range_ids: segment.ranges.clone(), clips });
        cursor += segment.duration;
    }
    draft.set_playhead(Tick::ZERO);
    let mut output = folder(s, p)?;
    let original = output.path.join("assembled-original.fcproj").to_str().ok_or_else(|| invalid("工程路径无效"))?.to_string();
    let editable = output.path.join("edit-copy.fcproj").to_str().ok_or_else(|| invalid("工程路径无效"))?.to_string();
    let preserved = output.path.join("previous-work.fcproj").to_str().ok_or_else(|| invalid("工程路径无效"))?.to_string();
    let bytes = filmcraft_format::encode_with_view(&s.project, Some(&s.project_view()), false);
    s.services.write_file(&preserved, &bytes).map_err(|e| invalid(format!("保存此前工作失败：{e}")))?;
    let assembly = Assembly {
        id: output.path.file_name().and_then(|n| n.to_str()).ok_or_else(|| invalid("组稿标识无效"))?.to_string(),
        source_item: item,
        sequence,
        categories,
        ranges,
        segments: mapping.clone(),
        original_project: original.clone(),
        edit_project: editable.clone(),
        preserved_project: preserved.clone(),
        parent_project: s.path.clone(),
    };
    draft.edit("记录组稿关联", |p, _| {
        p.assemblies.push(assembly.clone());
        Ok(())
    })?;
    draft.execute("sequence.settings", json!({"name":"组稿原稿"}))?;
    // The unique, newly reserved directory is the only place that can initially publish an original.
    // Normal save commands reject every original recorded in the project, including older assemblies.
    let bytes = filmcraft_format::encode_with_view(&draft.project, Some(&draft.project_view()), false);
    draft.services.write_file(&original, &bytes).map_err(|e| invalid(format!("保存组稿原稿失败：{e}")))?;
    draft.execute("sequence.settings", json!({"name":"组稿编辑副本"}))?;
    draft.execute("file.saveAs", json!({"path":editable}))?;
    if crate::selection_placement::source_signature(s, item)? != signature {
        return Err(invalid("组稿期间原片或媒体映射已变化，已有剪辑未改变"));
    }
    s.execute("file.open", json!({"path":editable}))?;
    // Opening may honour a preference to restore no tabs. Assembly explicitly opens its new sequence.
    s.execute("sequence.open", json!({"item":sequence.0}))?;
    s.execute("source.open", json!({"item":item.0}))?;
    output.committed = true;
    Ok(json!({"status":"assembled","sourceItem":item.0,"sequence":sequence.0,"clipCount":segments.len(),"duration":cursor.0,"durationSeconds":cursor.seconds(),
        "segments":mapping,"originalProject":original,"editProject":editable,"preservedProject":preserved}))
}

pub(crate) fn protected_original(project: &filmcraft_project::Project, path: &str) -> bool {
    project.assemblies.iter().any(|a| {
        a.original_project == path || std::fs::canonicalize(path).ok().zip(std::fs::canonicalize(&a.original_project).ok()).is_some_and(|(p, q)| p == q)
    })
}
fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    let current = s.project.assemblies.last();
    let role = current.map(|a| {
        if s.path.as_deref().is_some_and(|path| protected_original(&s.project, path)) {
            "original"
        } else if s.path.as_deref() == Some(a.edit_project.as_str()) {
            "editingCopy"
        } else {
            "derivedCopy"
        }
    });
    Ok(json!({"role":role,"currentProject":s.path,"assembly":current,"assemblies":s.project.assemblies}))
}
pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "selection.assemble",
            label: "按当前范围组稿",
            menu: &[],
            shortcut: None,
            params: r#"{"item":id?,"categories":["gunshot"|"voice"]?,"directory":str?,"projectToken":u64?}"#,
            enabled: always,
            run: assemble,
            journal: true,
        },
        CommandSpec {
            id: "selection.assembly",
            label: "查看组稿与副本关联",
            menu: &[],
            shortcut: None,
            params: "{}",
            enabled: always,
            run: inspect,
            journal: false,
        },
    ]
}
