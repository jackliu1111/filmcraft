use crate::Session;
use filmcraft_project::{ItemId, MediaRef};
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};
fn t(s: f64) -> i64 {
    Tick::from_seconds_f64(s).0
}
fn setup() -> (Session, String) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let path = format!("/private/tmp/filmcraft-event-mapping-{}-{:?}.mp4", std::process::id(), std::thread::current().id());
    std::fs::write(&path, b"event mapping fixture").unwrap();
    s.edit("Fixture", |p, _| {
        let m = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        m.media = MediaRef::File { path: path.clone() };
        m.info.duration = Tick(t(100.0));
        m.info.video.as_mut().unwrap().frame_rate = FrameRate::new(24, 1);
        Ok(())
    })
    .unwrap();
    let stat = std::fs::metadata(&path).unwrap();
    let guns = json!([
        {"event_id":"g1","start_s":7.0,"end_s":9.0},
        {"event_id":"g2","start_s":10.0,"end_s":12.0},
        {"event_id":"g3","start_s":15.0,"end_s":17.0},
        {"event_id":"g4","start_s":31.0,"end_s":32.0}]);
    let voices = json!([{ "event_id":"v1","start_s":9.0,"end_s":11.0 },{"event_id":"v2","start_s":33.0,"end_s":35.0}]);
    let result = json!({"schema_version":1,"time_unit":"seconds","result_id":"mapping-test","media":{
        "source":{"path":path,"size_bytes":stat.len(),"mtime_ns":stat.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64},
        "duration_s":100.0,"video":{"start_time_s":0.0},"selected_audio":{"stream_index":1},"audio_tracks":[{"stream_index":1,"sample_rate":48000,"channels":2}]},
        "events":guns,"candidates":[],"categories":{"gunshot":{"events":guns,"candidates":[]},"voice":{"events":voices,"candidates":[]}}});
    let job = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("recognition.attach", json!({"token":job["token"],"result":result})).unwrap();
    s.execute("file.newSequence", json!({"fps":24,"video":1,"audio":1})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    (s, path)
}
fn place(s: &mut Session, source: f64, duration: f64) -> Value {
    s.execute("timeline.place", json!({"item":5,"track":"V1","audioTrack":"A1","sourceIn":t(source),"duration":t(duration),"time":t(100.0)})).unwrap()
}
fn mappings(s: &mut Session) -> Value {
    s.execute("timeline.eventMapping", json!({})).unwrap()["mappings"].clone()
}
fn spans(q: &Value, event: &str) -> Vec<(i64, i64, i64, i64)> {
    q.as_array()
        .unwrap()
        .iter()
        .filter(|m| m["eventId"] == event)
        .map(|m| {
            (m["sourceStart"].as_i64().unwrap(), m["sourceEnd"].as_i64().unwrap(), m["sequenceStart"].as_i64().unwrap(), m["sequenceEnd"].as_i64().unwrap())
        })
        .collect()
}
#[test]
fn event_mapping_projects_all_categories_clips_boundaries_and_repeats_each_use() {
    let (mut s, path) = setup();
    let original = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    place(&mut s, 8.0, 8.0);
    let manual = s.execute("selection.addRange", json!({"item":5,"start":t(30.0),"duration":t(6.0)})).unwrap();
    let snapshot = s.execute("selection.capture", json!({"item":5,"rangeId":manual["rangeId"]})).unwrap();
    s.execute("selection.place", json!({"snapshot":snapshot,"append":true})).unwrap();
    place(&mut s, 8.0, 8.0);
    let history = s.execute("history.list", json!({})).unwrap();
    let q = mappings(&mut s);
    assert_eq!(q.as_array().unwrap().len(), 10, "A/V partners describe one use, not duplicate events");
    assert_eq!(spans(&q, "g1"), vec![(t(8.0), t(9.0), 0, t(1.0)), (t(8.0), t(9.0), t(14.0), t(15.0))]);
    assert_eq!(spans(&q, "g3"), vec![(t(15.0), t(16.0), t(7.0), t(8.0)), (t(15.0), t(16.0), t(21.0), t(22.0))]);
    assert_eq!(spans(&q, "g4"), vec![(t(31.0), t(32.0), t(9.0), t(10.0))]);
    assert_eq!(spans(&q, "v2"), vec![(t(33.0), t(35.0), t(11.0), t(13.0))]);
    assert!(q.as_array().unwrap().iter().all(|m| m["clips"].as_array().unwrap().len() == 2));
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), original);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn coordinates_distinguish_sequence_nine_from_source_thirty_one_and_return_every_repeat() {
    let (mut s, path) = setup();
    place(&mut s, 8.0, 8.0);
    place(&mut s, 30.0, 6.0);
    place(&mut s, 8.0, 8.0);
    s.execute("playhead.set", json!({"seconds":9.0})).unwrap();
    let history = s.execute("history.list", json!({})).unwrap();
    let coordinate = s.execute("timeline.coordinates", json!({"time":t(9.0)})).unwrap();
    assert_eq!(coordinate["coordinate"], "sequence");
    assert_eq!(coordinate["positions"].as_array().unwrap().len(), 1);
    assert_eq!(coordinate["positions"][0]["sequenceTime"], t(9.0));
    assert_eq!(coordinate["positions"][0]["sourceTime"], t(31.0));
    assert_eq!(coordinate["positions"][0]["clips"].as_array().unwrap().len(), 2);
    let reverse = s.execute("timeline.coordinates", json!({"item":5,"sourceTime":t(10.0)})).unwrap();
    assert_eq!(reverse["coordinate"], "source");
    assert_eq!(reverse["positions"].as_array().unwrap().iter().map(|p| p["sequenceTime"].as_i64().unwrap()).collect::<Vec<_>>(), vec![t(2.0), t(16.0)]);
    assert!(s.execute("timeline.coordinates", json!({"time":t(22.0)})).unwrap()["positions"].as_array().unwrap().is_empty(), "half-open clip end has no frame");
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    s.execute("sequence.matchFrame", json!({})).unwrap();
    let source = s.execute("source.inspect", json!({})).unwrap();
    assert_eq!(source["playhead"], t(31.0));
    assert_eq!(source["item"], 5);
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap()["playhead"], t(9.0));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn event_mapping_and_inverse_coordinates_follow_native_speed_reverse_and_frame_hold() {
    let (mut s, path) = setup();
    let clips = place(&mut s, 8.0, 8.0)["clips"].clone();
    s.execute("clip.speedDuration", json!({"clips":clips,"speed":200,"ripple":false})).unwrap();
    let q = mappings(&mut s);
    assert_eq!(spans(&q, "g2"), vec![(t(10.0), t(12.0), t(1.0), t(2.0))]);
    assert_eq!(s.execute("timeline.coordinates", json!({"time":t(1.0)})).unwrap()["positions"][0]["sourceTime"], t(10.0));
    assert_eq!(s.execute("timeline.coordinates", json!({"item":5,"sourceTime":t(10.0)})).unwrap()["positions"][0]["sequenceTime"], t(1.0));
    s.execute("clip.speedDuration", json!({"clips":clips,"speed":200,"reverse":true,"ripple":false})).unwrap();
    let q = mappings(&mut s);
    assert_eq!(spans(&q, "g1"), vec![(t(8.0), t(9.0), t(3.5), t(4.0))]);
    assert_eq!(spans(&q, "g3"), vec![(t(15.0), t(16.0), 0, t(0.5))]);
    assert_eq!(s.execute("timeline.coordinates", json!({"time":t(1.0)})).unwrap()["positions"][0]["sourceTime"], t(14.0) - 1);
    let inverse = s.execute("timeline.coordinates", json!({"item":5,"sourceTime":t(10.0)})).unwrap();
    assert!((inverse["positions"][0]["sequenceTime"].as_i64().unwrap() - t(3.0)).abs() <= 1);
    let video = clips[0].as_u64().unwrap();
    s.execute("clip.frameHoldOptions", json!({"clips":[video],"holdOn":"sourceTimecode","time":t(10.0)})).unwrap();
    let held = mappings(&mut s);
    let held = held.as_array().unwrap().iter().filter(|m| m["clips"].as_array().unwrap().contains(&json!(video))).collect::<Vec<_>>();
    assert_eq!(held.len(), 2, "only the gun and voice events covering the held source frame remain");
    assert!(held.iter().all(|m| m["sequenceStart"] == 0 && m["sequenceEnd"] == t(4.0)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn event_mapping_and_usage_recompute_after_trim_reorder_delete_and_the_full_undo_chain() {
    let (mut s, path) = setup();
    let first = place(&mut s, 8.0, 8.0)["clips"][0].as_u64().unwrap();
    let second = place(&mut s, 30.0, 6.0)["clips"][0].as_u64().unwrap();
    let before = mappings(&mut s);
    let events = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    let usage = |s: &mut Session| s.execute("selection.usage", json!({"item":5,"start":t(8.0),"duration":t(8.0)})).unwrap();
    assert_eq!(usage(&mut s)["status"], "used");
    s.execute("timeline.trim", json!({"clip":first,"edge":"out","mode":"regular","delta":t(-2.0)})).unwrap();
    let trimmed = mappings(&mut s);
    assert!(spans(&trimmed, "g3").is_empty());
    assert_eq!(spans(&trimmed, "g4"), vec![(t(31.0), t(32.0), t(7.0), t(8.0))]);
    assert_eq!(usage(&mut s)["status"], "partial");
    assert_eq!(usage(&mut s)["covered"], t(6.0));
    s.execute("timeline.move", json!({"moves":[{"clip":second,"track":"V1","time":0}]})).unwrap();
    let reordered = mappings(&mut s);
    assert_eq!(spans(&reordered, "g2"), vec![(t(10.0), t(12.0), t(8.0), t(10.0))]);
    assert_eq!(spans(&reordered, "g4"), vec![(t(31.0), t(32.0), t(1.0), t(2.0))]);
    s.execute("edit.clear", json!({"clips":[first]})).unwrap();
    let deleted = mappings(&mut s);
    assert!(spans(&deleted, "g2").is_empty());
    assert_eq!(usage(&mut s)["status"], "unused");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(mappings(&mut s), reordered);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(mappings(&mut s), trimmed);
    assert_eq!(usage(&mut s)["status"], "partial");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(mappings(&mut s), before);
    assert_eq!(usage(&mut s)["status"], "used");
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), events);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn saved_original_events_and_native_clips_rebuild_the_same_mapping_coordinates_and_usage() {
    let (mut s, path) = setup();
    let first = place(&mut s, 8.0, 8.0)["clips"][0].as_u64().unwrap();
    place(&mut s, 30.0, 6.0);
    place(&mut s, 8.0, 8.0);
    s.execute("timeline.trim", json!({"clip":first,"edge":"out","delta":t(-2.0)})).unwrap();
    let before = mappings(&mut s);
    let coordinates = s.execute("timeline.coordinates", json!({"item":5,"sourceTime":t(10.0)})).unwrap();
    let usage = s.execute("selection.usage", json!({"item":5,"start":t(8.0),"duration":t(8.0)})).unwrap();
    let events = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    let project_path = format!("{path}.fcproj");
    s.execute("file.saveAs", json!({"path":project_path})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":project_path})).unwrap();
    assert_eq!(mappings(&mut reopened), before);
    assert_eq!(reopened.execute("timeline.coordinates", json!({"item":5,"sourceTime":t(10.0)})).unwrap(), coordinates);
    assert_eq!(reopened.execute("selection.usage", json!({"item":5,"start":t(8.0),"duration":t(8.0)})).unwrap(), usage);
    assert_eq!(reopened.execute("recognition.inspect", json!({"item":5})).unwrap(), events);
    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(path).unwrap();
}
