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
