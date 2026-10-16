use super::*;
use serde_json::{Value, json};
fn recording() -> (Session, String) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let path = format!("/private/tmp/filmcraft-recognition-{}-{:?}.mp4", std::process::id(), std::thread::current().id());
    std::fs::write(&path, b"recognition fixture").unwrap();
    s.edit("Fixture", |p, _| {
        let m = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        m.media = filmcraft_project::MediaRef::File { path: path.clone() };
        m.info.duration = Tick::from_seconds_f64(100.0);
        Ok(())
    })
    .unwrap();
    (s, path)
}
fn result(path: &str) -> Value {
    let stat = std::fs::metadata(path).unwrap();
    json!({"schema_version":1,"time_unit":"seconds","result_id":"test-result", "media": {
      "source":{"path":path,"size_bytes":stat.len(),"mtime_ns":stat.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64},
      "duration_s":100.0, "video":{"start_time_s":0.0},
      "selected_audio":{"stream_index":1}, "audio_tracks":[{"stream_index":1,"sample_rate":48000,"channels":2}]
    }, "events":[{"start_s":10.0,"end_s":12.0,"label":"gunshot"}],
       "candidates":[{"start_s":5.0,"end_s":17.0}]})
}
#[test]
fn recognition_attaches_to_captured_source_without_assembling_or_changing_edits() {
    let (mut s, path) = recording();
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    let token = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    // More ordinary edits than the default undo history retains.
    for time in 0..205 {
        s.execute("project.setMarks", json!({"item":5,"in":time})).unwrap();
    }
    s.execute("recognition.attach", json!({"token":token["token"],"result":result(&path)})).unwrap();
    let recognition = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    assert_eq!(recognition["status"], "completed");
    assert_eq!(recognition["result"]["events"][0]["start_s"], 10.0);
    assert_eq!(recognition["result"]["candidates"][0]["start_s"], 5.0);
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recognition_rejects_mismatched_audio_parameters_without_touching_edits() {
    let (mut s, path) = recording();
    let before = s.execute("project.inspect", json!({})).unwrap();
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    let mut result = result(&path);
    result["media"]["audio_tracks"][0]["sample_rate"] = json!(8000);
    result["media"]["audio_tracks"][0]["channels"] = json!(1);
    assert!(s.execute("recognition.attach", json!({"token":job["token"],"result":result})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recognition_cancel_and_project_replacement_reject_late_results() {
    let (mut s, path) = recording();
    let result = result(&path);
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("recognition.cancel", json!({"token":job["token"]})).unwrap();
    let before = s.execute("project.inspect", json!({})).unwrap();
    assert!(s.execute("recognition.attach", json!({"token":job["token"],"result":result})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("file.newProject", json!({"name":"Other"})).unwrap();
    let before = s.execute("project.inspect", json!({})).unwrap();
    assert!(s.execute("recognition.attach", json!({"token":job["token"],"result":result})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recognition_survives_real_disk_save_and_open_but_pending_task_does_not() {
    let (mut s, path) = recording();
    let result = result(&path);
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("source.open", json!({"item":6})).unwrap();
    s.execute("recognition.attach", json!({"token":job["token"],"result":result})).unwrap();
    assert_eq!(s.execute("state.inspect", json!({})).unwrap()["source_item"], 6);
    let saved = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    let pending = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    let project_path = format!("{path}.fcproj");
    s.execute("file.saveAs", json!({"path":project_path})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":project_path})).unwrap();
    assert_eq!(reopened.execute("recognition.inspect", json!({"item":5})).unwrap(), saved);
    assert!(reopened.execute("recognition.attach", json!({"token":pending["token"],"result":result})).is_err());
    s.execute("file.open", json!({"path":project_path})).unwrap();
    assert!(s.execute("recognition.attach", json!({"token":pending["token"],"result":result})).is_err());
    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recognition_rejects_voice_category_outside_source_even_when_legacy_gun_events_are_valid() {
    let (mut s, path) = recording();
    let before = s.execute("project.inspect", json!({})).unwrap();
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    let mut result = result(&path);
    result["categories"] = json!({"voice":{"status":"completed","events":[{"start_s":99.0,"end_s":101.0}],"candidates":[]}});
    assert!(s.execute("recognition.attach", json!({"token":job["token"],"result":result})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recognition_preserves_overlapping_voice_events_policy_and_unavailable_status_through_disk_open() {
    let (mut s, path) = recording();
    let mut r = result(&path);
    r["categories"] = json!({
        "gunshot":{"status":"completed","events":r["events"],"candidates":r["candidates"]},
        "voice":{"status":"completed","postprocess":{"version":"filmcraft-spoken-voice-v1","candidate_padding_s":1.0},
          "events":[{"event_id":"voice-1","start_s":11.0,"end_s":13.0}],
          "candidates":[{"candidate_id":"voice-1:range","start_s":10.0,"end_s":14.0}]}
    });
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("recognition.attach", json!({"token":job["token"],"result":r})).unwrap();
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), before);
    let project_path = format!("{path}.voice.fcproj");
    s.execute("file.saveAs", json!({"path":project_path})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":project_path})).unwrap();
    assert_eq!(reopened.execute("recognition.inspect", json!({"item":5})).unwrap()["result"], r);
    let job = reopened.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    r["categories"]["voice"] = json!({"status":"unavailable","reason":"missing_class_scores","events":[],"candidates":[]});
    reopened.execute("recognition.attach", json!({"token":job["token"],"result":r})).unwrap();
    reopened.execute("file.save", json!({})).unwrap();
    let mut missing_reopened = Session::default();
    missing_reopened.execute("file.open", json!({"path":project_path})).unwrap();
    assert_eq!(missing_reopened.execute("recognition.inspect", json!({"item":5})).unwrap()["result"]["categories"]["voice"]["status"], "unavailable");
    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn multitrack_analysis_maps_container_indices_and_preserves_all_retained_streams_after_save_open() {
    let (mut s, path) = recording();
    s.edit("Second audio fixture", |p, _| {
        let media = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        let mut audio = media.info.audio_streams[0].clone();
        audio.channels = 1;
        audio.sample_rate = 16000;
        media.info.audio_streams.push(audio);
        Ok(())
    })
    .unwrap();
    let mut r = result(&path);
    r["media"]["audio_tracks"] = json!([
        {"stream_index":2,"sample_rate":48000,"channels":2,"start_time_s":-0.021},
        {"stream_index":4,"sample_rate":16000,"channels":1,"start_time_s":0.25}
    ]);
    r["media"]["selected_audio"] = json!({"stream_index":4,"video_origin_s":0.0,"audio_origin_s":0.25,"audio_to_video_offset_s":0.25});
    let map = s.execute("recognition.mapAudio", json!({"item":5,"media":r["media"]})).unwrap();
    assert_eq!(map["audioMapping"][1]["containerStream"], 4);
    assert_eq!(map["audioMapping"][1]["nativeOrdinal"], 1);
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":4,"media":r["media"]})).unwrap();
    s.execute("recognition.attach", json!({"token":job["token"],"result":r})).unwrap();
    let association = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    assert_eq!(association["analysisAudioOrdinal"], 1);
    assert_eq!(association["audioStream"], 4);
    s.execute("file.newSequence", json!({"name":"Retained audio","video":1,"audio":1})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    s.execute("timeline.place", json!({"item":5,"track":"V1","audioTrack":"A1","sourceIn":0,"duration":Tick::from_seconds_f64(2.0).0,"time":0})).unwrap();
    let sequence = s.execute("sequence.inspect", json!({})).unwrap();
    let streams: Vec<_> =
        sequence["audio"].as_array().unwrap().iter().flat_map(|t| t["items"].as_array().unwrap()).map(|c| c["audioStream"].as_u64().unwrap()).collect();
    assert_eq!(streams, vec![0, 1], "analysis choice must not discard other sounds");
    let video = &sequence["video"][0]["items"][0];
    for track in sequence["audio"].as_array().unwrap() {
        for clip in track["items"].as_array().unwrap() {
            assert_eq!(clip["link"], video["link"]);
            assert_eq!(clip["sourceIn"], video["sourceIn"]);
            assert_eq!(clip["duration"], video["duration"]);
            assert_eq!(clip["start"], video["start"]);
        }
    }
    let saved = format!("{path}.multitrack.fcproj");
    s.execute("file.saveAs", json!({"path":saved})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":saved})).unwrap();
    assert_eq!(reopened.execute("recognition.inspect", json!({"item":5})).unwrap(), association);
    assert_eq!(reopened.execute("sequence.inspect", json!({})).unwrap()["audio"], sequence["audio"]);
    std::fs::remove_file(saved).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn multitrack_analysis_rejects_ambiguous_mapping_wrong_selection_and_unsupported_origins_without_mutation() {
    let (mut s, path) = recording();
    s.edit("Two audio streams", |p, _| {
        let media = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        media.info.audio_streams.push(media.info.audio_streams[0].clone());
        Ok(())
    })
    .unwrap();
    let mut r = result(&path);
    r["media"]["audio_tracks"] = json!([
        {"stream_index":1,"sample_rate":48000,"channels":2,"start_time_s":-0.021},
        {"stream_index":3,"sample_rate":48000,"channels":2,"start_time_s":0.125}
    ]);
    r["media"]["selected_audio"] = json!({"stream_index":3,"video_origin_s":0.0,"audio_origin_s":0.125,"audio_to_video_offset_s":0.125});
    let before = s.execute("project.inspect", json!({})).unwrap();
    assert!(s.execute("recognition.begin", json!({"item":5,"audioStream":3})).is_err());
    for invalid_media in [
        {
            let mut media = r["media"].clone();
            media["audio_tracks"][1]["stream_index"] = json!(1);
            media
        },
        {
            let mut media = r["media"].clone();
            media["audio_tracks"][1]["sample_rate"] = json!(16000);
            media
        },
        {
            let mut media = r["media"].clone();
            media["audio_tracks"][0]["start_time_s"] = Value::Null;
            media
        },
        {
            let mut media = r["media"].clone();
            media["video"]["start_time_s"] = json!(1.0);
            media
        },
    ] {
        assert!(s.execute("recognition.mapAudio", json!({"item":5,"media":invalid_media})).is_err());
    }
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":3,"media":r["media"]})).unwrap();
    let mut wrong_track = r.clone();
    wrong_track["media"]["selected_audio"]["stream_index"] = json!(1);
    assert!(s.execute("recognition.attach", json!({"token":job["token"],"result":wrong_track})).is_err());
    let mut wrong_offset = r.clone();
    wrong_offset["media"]["selected_audio"]["audio_to_video_offset_s"] = json!(0.0);
    assert!(s.execute("recognition.attach", json!({"token":job["token"],"result":wrong_offset})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), Value::Null);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn analysis_queue_orders_deduplicates_and_cancels_without_taking_edit_undo() {
    let (mut s, path) = recording();
    let history = s.execute("history.list", json!({})).unwrap();
    let first = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    let same = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    assert_eq!(first["id"], same["id"]);
    let next = s.execute("analysis.next", json!({})).unwrap();
    assert_eq!(next["id"], first["id"]);
    let running = s.execute("analysis.start", json!({"job":first["id"],"attempt":first["attempt"]})).unwrap();
    assert_eq!(running["status"], "probing");
    assert!(s.execute("analysis.next", json!({})).unwrap().is_null());
    s.execute("analysis.cancel", json!({"job":first["id"]})).unwrap();
    assert!(s.execute("analysis.probed", json!({"job":first["id"],"attempt":first["attempt"],"media":result(&path)["media"]})).is_err());
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    assert!(s.execute("recognition.inspect", json!({"item":5})).unwrap().is_null());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn analysis_queue_skips_waiting_audio_then_attaches_only_the_correct_source_and_survives_failure_retry() {
    let (mut s, path) = recording();
    s.edit("Queue fixtures", |p, _| {
        let original = p.item(ItemId(5)).unwrap().as_media().unwrap().clone();
        *p.item_mut(ItemId(6)).unwrap().as_media_mut().unwrap() = original.clone();
        let multi = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        multi.info.audio_streams.push(original.info.audio_streams[0].clone());
        Ok(())
    })
    .unwrap();
    let first = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    let second = s.execute("analysis.enqueue", json!({"item":6})).unwrap();
    assert!(s.execute("analysis.start", json!({"job":second["id"],"attempt":1})).is_err(), "explicit claims must follow the same ready order");
    s.execute("analysis.start", json!({"job":first["id"],"attempt":1})).unwrap();
    let mut multi = result(&path)["media"].clone();
    multi["audio_tracks"] = json!([
        {"stream_index":1,"sample_rate":48000,"channels":2,"start_time_s":0.0},
        {"stream_index":3,"sample_rate":48000,"channels":2,"start_time_s":0.0}]);
    assert_eq!(s.execute("analysis.probed", json!({"job":first["id"],"attempt":1,"media":multi})).unwrap()["status"], "waiting_audio");
    assert_eq!(s.execute("analysis.next", json!({})).unwrap()["id"], second["id"]);
    s.execute("analysis.start", json!({"job":second["id"],"attempt":1})).unwrap();
    s.execute("analysis.probed", json!({"job":second["id"],"attempt":1,"media":result(&path)["media"]})).unwrap();
    assert_eq!(s.execute("analysis.start", json!({"job":second["id"],"attempt":1})).unwrap()["status"], "running");
    s.execute("project.setMarks", json!({"item":6,"in":10})).unwrap();
    let sequence = s.execute("sequence.inspect", json!({})).unwrap();
    let complete = s.execute("analysis.complete", json!({"job":second["id"],"attempt":1,"result":result(&path)})).unwrap();
    assert_eq!(complete["status"], "completed");
    assert!(s.execute("recognition.inspect", json!({"item":5})).unwrap().is_null());
    let completed_record = s.execute("recognition.inspect", json!({"item":6})).unwrap();
    assert_eq!(completed_record["status"], "completed");
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), sequence);
    s.execute("analysis.selectAudio", json!({"job":first["id"],"attempt":1,"audioStream":3})).unwrap();
    s.execute("analysis.start", json!({"job":first["id"],"attempt":1})).unwrap();
    assert_eq!(s.execute("analysis.fail", json!({"job":first["id"],"attempt":1,"error":"bridge exited"})).unwrap()["status"], "failed");
    let retried = s.execute("analysis.retry", json!({"job":first["id"]})).unwrap();
    assert_eq!(retried["attempt"], 2);
    assert_eq!(retried["status"], "queued");
    assert!(s.execute("analysis.complete", json!({"job":first["id"],"attempt":1,"result":result(&path)})).is_err());
    assert_eq!(s.execute("analysis.list", json!({})).unwrap().as_array().unwrap().len(), 2);
    assert_eq!(s.execute("recognition.inspect", json!({"item":6})).unwrap(), completed_record);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn analysis_queue_reopens_interrupted_without_auto_dispatch_and_cancel_does_not_rewind_on_edit_undo() {
    let (mut s, path) = recording();
    let task = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    s.execute("analysis.start", json!({"job":task["id"],"attempt":1})).unwrap();
    s.execute("analysis.probed", json!({"job":task["id"],"attempt":1,"media":result(&path)["media"]})).unwrap();
    s.execute("analysis.start", json!({"job":task["id"],"attempt":1})).unwrap();
    let saved = format!("{path}.queue.fcproj");
    s.execute("file.saveAs", json!({"path":saved})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":saved})).unwrap();
    let restored = reopened.execute("analysis.list", json!({})).unwrap();
    assert_eq!(restored[0]["status"], "interrupted");
    assert_eq!(restored[0]["previous_status"], "running");
    assert!(reopened.execute("analysis.next", json!({})).unwrap().is_null());
    assert!(reopened.execute("analysis.complete", json!({"job":task["id"],"attempt":1,"result":result(&path)})).is_err());
    let retry = reopened.execute("analysis.retry", json!({"job":task["id"]})).unwrap();
    assert_eq!(retry["attempt"], 2);
    reopened.execute("project.setMarks", json!({"item":5,"in":20})).unwrap();
    reopened.execute("analysis.cancel", json!({"job":task["id"]})).unwrap();
    reopened.execute("edit.undo", json!({})).unwrap();
    assert_eq!(reopened.execute("analysis.list", json!({})).unwrap()[0]["status"], "cancelled");
    reopened.execute("edit.redo", json!({})).unwrap();
    assert_eq!(reopened.execute("analysis.list", json!({})).unwrap()[0]["status"], "cancelled");
    std::fs::remove_file(saved).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn analysis_queue_distinguishes_valid_zero_candidates_from_unavailable_and_keeps_results_after_retry_cancel() {
    let (mut s, path) = recording();
    let task = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    s.execute("analysis.start", json!({"job":task["id"],"attempt":1})).unwrap();
    s.execute("analysis.probed", json!({"job":task["id"],"attempt":1,"media":result(&path)["media"]})).unwrap();
    s.execute("analysis.start", json!({"job":task["id"],"attempt":1})).unwrap();
    let mut zero = result(&path);
    zero["events"] = json!([]);
    zero["candidates"] = json!([]);
    assert_eq!(s.execute("analysis.complete", json!({"job":task["id"],"attempt":1,"result":zero})).unwrap()["status"], "completed");
    let old = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    assert_eq!(old["result"]["events"], json!([]));
    s.execute("analysis.retry", json!({"job":task["id"]})).unwrap();
    s.execute("analysis.cancel", json!({"job":task["id"]})).unwrap();
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), old);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(s.execute("analysis.retry", json!({"job":task["id"]})).unwrap()["status"], "unavailable");
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), old);
}

#[test]
fn analysis_queue_marks_missing_runtime_unavailable_without_inventing_a_zero_result() {
    let (mut s, path) = recording();
    let task = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    s.execute("analysis.start", json!({"job":task["id"],"attempt":1})).unwrap();
    let state = s.execute("analysis.fail", json!({"job":task["id"],"attempt":1,"code":"backend_unavailable","error":"共享分析服务不存在"})).unwrap();
    assert_eq!(state["status"], "unavailable");
    assert_eq!(state["error"], "共享分析服务不存在");
    assert!(s.execute("recognition.inspect", json!({"item":5})).unwrap().is_null());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn analysis_queue_updates_persist_without_invalidating_edit_previews() {
    let (mut s, path) = recording();
    let before = s.execute("project.inspect", json!({})).unwrap();
    let edit_revision = before["editingRevision"].as_u64().expect("public edit-preview revision");
    s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    let after = s.execute("project.inspect", json!({})).unwrap();
    assert!(after["revision"].as_u64().unwrap() > before["revision"].as_u64().unwrap());
    assert_eq!(after["editingRevision"], edit_revision);
    s.execute("project.setMarks", json!({"item":5,"in":30})).unwrap();
    assert!(s.execute("project.inspect", json!({})).unwrap()["editingRevision"].as_u64().unwrap() > edit_revision);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn analysis_queue_binds_the_persisted_source_identity_even_if_item_ids_are_reused() {
    let (mut s, path) = recording();
    let task = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    std::fs::write(&path, b"different content and size").unwrap();
    assert_eq!(
        s.execute("analysis.start", json!({"job":task["id"],"attempt":1})).unwrap()["status"],
        "unavailable",
        "a queued job must not silently adopt a changed source"
    );
    let replacement = format!("{path}.replacement.mp4");
    std::fs::write(&replacement, b"replacement recording").unwrap();
    s.edit("Replace item with reused ID", |p, _| {
        p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap().media = filmcraft_project::MediaRef::File { path: replacement.clone() };
        Ok(())
    })
    .unwrap();
    let new = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    assert_ne!(new["id"], task["id"]);
    let old_retry = s.execute("analysis.retry", json!({"job":task["id"]})).unwrap();
    assert_eq!(old_retry["status"], "unavailable");
    assert_eq!(old_retry["source_path"], path);
    assert_eq!(s.execute("analysis.next", json!({})).unwrap()["id"], new["id"]);
    assert_eq!(s.execute("analysis.start", json!({"job":new["id"],"attempt":1})).unwrap()["status"], "probing");
    assert!(s.execute("analysis.probed", json!({"job":task["id"],"attempt":1,"media":result(&path)["media"]})).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(replacement).unwrap();
}

#[test]
fn analysis_queue_removed_running_source_invalidates_its_token_without_blocking_other_materials() {
    let (mut s, path) = recording();
    let other = format!("{path}.other.mp4");
    std::fs::write(&other, b"independent source").unwrap();
    s.edit("Independent source fixture", |p, _| {
        let mut media = p.item(ItemId(5)).unwrap().as_media().unwrap().clone();
        media.media = filmcraft_project::MediaRef::File { path: other.clone() };
        *p.item_mut(ItemId(6)).unwrap().as_media_mut().unwrap() = media;
        Ok(())
    })
    .unwrap();
    let first = s.execute("analysis.enqueue", json!({"item":5})).unwrap();
    let second = s.execute("analysis.enqueue", json!({"item":6})).unwrap();
    s.execute("analysis.start", json!({"job":first["id"],"attempt":1})).unwrap();
    s.execute("analysis.probed", json!({"job":first["id"],"attempt":1,"media":result(&path)["media"]})).unwrap();
    s.execute("analysis.start", json!({"job":first["id"],"attempt":1})).unwrap();
    s.edit("Remove imported item", |p, _| {
        p.items.remove(&ItemId(5));
        Ok(())
    })
    .unwrap();
    assert!(s.execute("analysis.checkOrigin", json!({"job":first["id"]})).is_err());
    assert_eq!(
        s.execute("analysis.fail", json!({"job":first["id"],"attempt":1,"code":"source_changed","error":"原片已移除"})).unwrap()["status"],
        "unavailable"
    );
    assert!(s.execute("analysis.complete", json!({"job":first["id"],"attempt":1,"result":result(&path)})).is_err());
    assert_eq!(s.execute("analysis.next", json!({})).unwrap()["id"], second["id"]);
    assert_eq!(s.execute("analysis.start", json!({"job":second["id"],"attempt":1})).unwrap()["status"], "probing");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(other).unwrap();
}
