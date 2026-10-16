use super::*;
use serde_json::{Value, json};
fn ready() -> (Session, String) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let path = format!("/private/tmp/filmcraft-selection-{}-{:?}.mp4", std::process::id(), std::thread::current().id());
    std::fs::write(&path, b"selection fixture").unwrap();
    s.edit("Fixture", |p, _| {
        let m = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        m.media = filmcraft_project::MediaRef::File { path: path.clone() };
        m.info.duration = Tick::from_seconds_f64(100.0);
        m.info.video.as_mut().unwrap().frame_rate = FrameRate::new(24, 1);
        Ok(())
    })
    .unwrap();
    let st = std::fs::metadata(&path).unwrap();
    let result = json!({"schema_version":1,"time_unit":"seconds","result_id":"selection-test", "media":{
      "source":{"path":path,"size_bytes":st.len(),"mtime_ns":st.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64},
      "duration_s":100.0,"video":{"start_time_s":0.0},"selected_audio":{"stream_index":1},"audio_tracks":[{"stream_index":1,"sample_rate":48000,"channels":2}]},
      "events":[{"event_id":"g1","start_s":10.0,"end_s":12.0}],"candidates":[{"start_s":5.0,"end_s":17.0}]});
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("recognition.attach", json!({"token":job["token"],"result":result})).unwrap();
    (s, path)
}
fn inspect(s: &mut Session) -> Value {
    s.execute("selection.inspect", json!({"item":5})).unwrap()
}
#[test]
fn selection_defaults_are_padded_independently_from_immutable_model_events() {
    let (mut s, path) = ready();
    let ranges = inspect(&mut s);
    assert_eq!(ranges["ranges"][0]["start"], Tick::from_seconds_f64(5.0).0);
    assert_eq!(ranges["ranges"][0]["duration"], Tick::from_seconds_f64(12.0).0);
    assert_eq!(ranges["ranges"][0]["category"], "gunshot");
    assert_eq!(ranges["ranges"][0]["eventIds"], json!(["g1"]));
    assert_eq!(ranges["ranges"][0]["manual"], false);
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap()["result"]["events"][0]["start_s"], 10.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selection_boundary_edit_is_one_native_undo_step_and_never_changes_model_events() {
    let (mut s, path) = ready();
    let initial = inspect(&mut s);
    let id = &initial["ranges"][0]["rangeId"];
    let events = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    let sequence = s.execute("sequence.inspect", json!({})).unwrap();
    let history = s.execute("history.list", json!({})).unwrap()["undo"].as_array().unwrap().len();
    s.execute("selection.setRange",json!({"item":5,"rangeId":id,"start":Tick::from_seconds_f64(8.0).0,"duration":Tick::from_seconds_f64(8.0).0,"expectedRevision":0,"projectToken":initial["projectToken"]})).unwrap();
    assert_eq!(inspect(&mut s)["ranges"][0]["startSeconds"], 8.0);
    assert_eq!(inspect(&mut s)["ranges"][0]["endSeconds"], 16.0);
    assert_eq!(inspect(&mut s)["ranges"][0]["revision"], 1);
    assert_eq!(s.execute("history.list", json!({})).unwrap()["undo"].as_array().unwrap().len(), history + 1);
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), events);
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), sequence);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(inspect(&mut s), initial);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(inspect(&mut s)["ranges"][0]["endSeconds"], 16.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selection_manual_supplement_has_no_invented_event_and_survives_save_open() {
    let (mut s, path) = ready();
    let events = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    let added = s
        .execute("selection.addRange", json!({"item":5,"start":Tick::from_seconds_f64(30.0).0,"duration":Tick::from_seconds_f64(6.0).0,"category":"gunshot"}))
        .unwrap();
    assert_eq!(added["manual"], true);
    assert_eq!(added["eventIds"], json!([]));
    assert_eq!(added["resultId"], Value::Null);
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), events);
    let saved = inspect(&mut s)["ranges"].clone();
    let project_path = format!("{path}.fcproj");
    s.execute("file.saveAs", json!({"path":project_path})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":project_path})).unwrap();
    assert_eq!(inspect(&mut reopened)["ranges"], saved);
    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selection_rejects_invalid_or_stale_commits_and_snaps_valid_boundaries_to_frames() {
    let (mut s, path) = ready();
    let initial = inspect(&mut s);
    let id = initial["ranges"][0]["rangeId"].clone();
    s.execute(
        "selection.setRange",
        json!({"item":5,"rangeId":id,"start":Tick::from_seconds_f64(8.0).0+1,"duration":Tick::from_seconds_f64(8.0).0,"expectedRevision":0}),
    )
    .unwrap();
    assert_eq!(inspect(&mut s)["ranges"][0]["startSeconds"], 8.0);
    assert_eq!(inspect(&mut s)["ranges"][0]["endSeconds"], 16.0);
    let before = inspect(&mut s);
    let project = s.execute("project.inspect", json!({})).unwrap();
    for (start, duration) in [(-1, 10), (0, 0), (0, -1), (Tick::from_seconds_f64(99.0).0, Tick::from_seconds_f64(2.0).0), (i64::MAX - 10, 100), (0, 1)] {
        assert!(s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":start,"duration":duration,"expectedRevision":1})).is_err());
        assert_eq!(inspect(&mut s), before);
        assert_eq!(s.execute("project.inspect", json!({})).unwrap(), project);
    }
    assert!(s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":0,"duration":Tick::from_seconds_f64(1.0).0,"expectedRevision":0})).is_err());
    assert!(s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":0,"duration":Tick::from_seconds_f64(1.0).0,"projectToken":0})).is_err());
    assert_eq!(inspect(&mut s), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selection_legacy_completed_record_derives_defaults_then_persists_first_edit() {
    let (mut s, path) = ready();
    let project_path = format!("{path}.fcproj");
    s.execute("file.saveAs", json!({"path":project_path})).unwrap();
    // External file fixture emulates an existing #21 project, before range metadata existed.
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(&project_path).unwrap()).unwrap();
    legacy["project"].as_object_mut().unwrap().remove("selection_ranges");
    std::fs::write(&project_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut opened = Session::default();
    opened.execute("file.open", json!({"path":project_path})).unwrap();
    let defaults = inspect(&mut opened);
    assert_eq!(defaults["ranges"][0]["startSeconds"], 5.0);
    assert_eq!(defaults["ranges"][0]["endSeconds"], 17.0);
    opened
        .execute(
            "selection.setRange",
            json!({"item":5,"rangeId":defaults["ranges"][0]["rangeId"],"start":Tick::from_seconds_f64(8.0).0,"duration":Tick::from_seconds_f64(8.0).0}),
        )
        .unwrap();
    opened.execute("file.save", json!({})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":project_path})).unwrap();
    assert_eq!(inspect(&mut reopened)["ranges"][0]["startSeconds"], 8.0);
    assert_eq!(inspect(&mut reopened)["ranges"][0]["endSeconds"], 16.0);
    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selection_range_and_timeline_edits_share_the_same_chronological_history() {
    let (mut s, path) = ready();
    s.execute("file.newSequence", json!({"width":1920,"height":1080,"fps":24,"video":1,"audio":1})).unwrap();
    let id = inspect(&mut s)["ranges"][0]["rangeId"].clone();
    s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":Tick::from_seconds_f64(8.0).0,"duration":Tick::from_seconds_f64(8.0).0})).unwrap();
    s.execute(
        "timeline.place",
        json!({"item":5,"track":"V1","audioTrack":"A1","time":0,"sourceIn":Tick::from_seconds_f64(8.0).0,"duration":Tick::from_seconds_f64(8.0).0}),
    )
    .unwrap();
    let placed = s.execute("sequence.inspect", json!({})).unwrap();
    s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":Tick::from_seconds_f64(7.0).0,"duration":Tick::from_seconds_f64(11.0).0})).unwrap();
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), placed);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(inspect(&mut s)["ranges"][0]["startSeconds"], 8.0);
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), placed);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap()["duration"], 0);
    assert_eq!(inspect(&mut s)["ranges"][0]["startSeconds"], 8.0);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(inspect(&mut s)["ranges"][0]["startSeconds"], 5.0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn selection_category_defaults_clamp_at_source_bounds_and_refresh_preserves_user_work() {
    let (mut s, path) = ready();
    let id = inspect(&mut s)["ranges"][0]["rangeId"].clone();
    s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":Tick::from_seconds_f64(8.0).0,"duration":Tick::from_seconds_f64(8.0).0})).unwrap();
    s.execute("selection.addRange", json!({"item":5,"start":Tick::from_seconds_f64(30.0).0,"duration":Tick::from_seconds_f64(2.0).0})).unwrap();
    let mut result = s.execute("recognition.inspect", json!({"item":5})).unwrap()["result"].clone();
    result["categories"] = json!({"voice":{"status":"completed","postprocess":{"version":"filmcraft-spoken-voice-v1"},"events":[
        {"event_id":"v1","start_s":20.0,"end_s":21.0},
        {"event_id":"v2","start_s":0.25,"end_s":0.75},
        {"event_id":"v3","start_s":99.5,"end_s":99.75}
    ]}});
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("recognition.attach", json!({"token":job["token"],"result":result})).unwrap();
    let current = inspect(&mut s);
    let ranges = current["ranges"].as_array().unwrap();
    let gun = ranges.iter().find(|r| r["rangeId"] == id).unwrap();
    assert_eq!(gun["startSeconds"], 8.0);
    assert_eq!(gun["endSeconds"], 16.0);
    assert!(ranges.iter().any(|r| r["manual"] == true && r["startSeconds"] == 30.0));
    let voice = ranges.iter().find(|r| r["eventIds"] == json!(["v1"])).unwrap();
    assert_eq!(voice["startSeconds"], 19.0);
    assert_eq!(voice["endSeconds"], 22.0);
    assert_eq!(ranges.iter().find(|r| r["eventIds"] == json!(["v2"])).unwrap()["startSeconds"], 0.0);
    assert_eq!(ranges.iter().find(|r| r["eventIds"] == json!(["v3"])).unwrap()["endSeconds"], 100.0);
    std::fs::remove_file(path).unwrap();
}
