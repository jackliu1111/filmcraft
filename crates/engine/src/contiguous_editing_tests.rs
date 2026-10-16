//! The public editing command boundary: source ranges remain independent of program placement.
use crate::Session;
use filmcraft_time::Tick;
use serde_json::{Value, json};

fn ticks(s: f64) -> i64 {
    Tick::from_seconds_f64(s).0
}
fn setup() -> (Session, u64) {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name":"Editing","fps":24,"video":2,"audio":2})).unwrap();
    let item = s.execute("file.newOfflineFile", json!({"name":"Original","video":true,"audio":true,"fps":24,"seconds":60})).unwrap()["item"].as_u64().unwrap();
    (s, item)
}
fn place(s: &mut Session, item: u64, source: f64, duration: f64, at: f64) {
    s.execute("timeline.place", json!({"item":item,"track":"V1","audioTrack":"A1","sourceIn":ticks(source),"duration":ticks(duration),"time":ticks(at)}))
        .unwrap();
}
fn spans(q: &Value, kind: &str) -> Vec<(i64, i64, i64)> {
    q[kind][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["start"].as_i64().unwrap(), c["duration"].as_i64().unwrap(), c["sourceIn"].as_i64().unwrap()))
        .collect()
}

#[test]
fn contiguous_target_appends_and_inserts_linked_media_as_one_undo() {
    let (mut s, item) = setup();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    place(&mut s, item, 8.0, 8.0, 40.0);
    place(&mut s, item, 30.0, 6.0, 50.0);
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    let expected = vec![(0, ticks(8.0), ticks(8.0)), (ticks(8.0), ticks(6.0), ticks(30.0))];
    assert_eq!(spans(&before, "video"), expected);
    assert_eq!(spans(&before, "audio"), expected);
    place(&mut s, item, 20.0, 3.0, 8.0);
    let inserted = s.execute("sequence.inspect", json!({})).unwrap();
    let expected = vec![(0, ticks(8.0), ticks(8.0)), (ticks(8.0), ticks(3.0), ticks(20.0)), (ticks(11.0), ticks(6.0), ticks(30.0))];
    assert_eq!(spans(&inserted, "video"), expected);
    assert_eq!(spans(&inserted, "audio"), expected);
    for (v, a) in inserted["video"][0]["items"].as_array().unwrap().iter().zip(inserted["audio"][0]["items"].as_array().unwrap()) {
        assert!(!v["link"].is_null());
        assert_eq!(v["link"], a["link"]);
        assert_eq!(v["item"], json!(item));
    }
    s.execute("edit.undo", json!({})).unwrap();
    let undone = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(undone["video"], before["video"]);
    assert_eq!(undone["audio"], before["audio"]);
    s.execute("edit.redo", json!({})).unwrap();
    let redone = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(redone["video"], inserted["video"]);
    assert_eq!(redone["audio"], inserted["audio"]);
}

#[test]
fn contiguous_preview_is_pure_and_commit_keeps_other_tracks_in_place() {
    let (mut s, item) = setup();
    s.execute("timeline.place", json!({"item":item,"track":"V2","audioTrack":"A2","time":ticks(6.0),"duration":ticks(4.0)})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    place(&mut s, item, 8.0, 8.0, 40.0);
    place(&mut s, item, 30.0, 6.0, 50.0);
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    let history = s.execute("history.list", json!({})).unwrap();
    let params = json!({"item":item,"track":"V1","audioTrack":"A1","sourceIn":ticks(20.0),"duration":ticks(3.0),"time":ticks(8.2)});
    let plan = s.execute("timeline.planPlacement", params.clone()).unwrap();
    assert_eq!(plan["time"], json!(ticks(8.0)));
    assert_eq!(plan["duration"], json!(ticks(3.0)));
    assert_eq!(plan["sourceIn"], json!(ticks(20.0)));
    assert_eq!(plan["shifts"].as_array().unwrap().len(), 2);
    for moved in plan["shifts"].as_array().unwrap() {
        assert_eq!(moved["from"], json!(ticks(8.0)));
        assert_eq!(moved["to"], json!(ticks(11.0)));
    }
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), before);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    s.execute("timeline.place", params).unwrap();
    let after = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(after["video"][1], before["video"][1]);
    assert_eq!(after["audio"][1], before["audio"][1]);
    assert_eq!(spans(&after, "video")[1], (ticks(8.0), ticks(3.0), ticks(20.0)));
}

#[test]
fn contiguous_retains_channel_clips_without_taking_an_unrelated_audio_track() {
    let (mut s, item) = setup();
    s.execute("timeline.place", json!({"item":item,"track":"V2","audioTrack":"A2","time":ticks(9.0),"duration":ticks(5.0)})).unwrap();
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    s.execute("clip.audioChannels", json!({"items":[item],"format":"mono","clips":[[0],[1]]})).unwrap();
    place(&mut s, item, 8.0, 8.0, 40.0);
    place(&mut s, item, 30.0, 6.0, 50.0);
    place(&mut s, item, 20.0, 3.0, 8.0);
    let q = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(q["editingTrack"]["audio"].as_array().unwrap().len(), 2);
    assert_eq!(q["audio"][1], before["audio"][1]);
    assert_eq!(q["audio"].as_array().unwrap().len(), 3);
    assert_eq!(q["audio"][0]["items"].as_array().unwrap().len(), 3);
    assert_eq!(q["audio"][2]["items"].as_array().unwrap().len(), 3);
    for (i, c) in q["audio"][2]["items"].as_array().unwrap().iter().enumerate() {
        assert_eq!(c["start"], q["video"][0]["items"][i]["start"]);
        assert_eq!(c["sourceIn"], q["video"][0]["items"][i]["sourceIn"]);
        assert_eq!(c["link"], q["video"][0]["items"][i]["link"]);
    }
    s.execute("sequence.setEditingTrack", json!({"enabled":false})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    let adopted = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(adopted["editingTrack"]["audio"].as_array().unwrap().len(), 2, "adopting existing linked A/V must retain every linked audio destination");
}

#[test]
fn contiguous_locked_linked_audio_refuses_preview_and_commit_without_a_history_step() {
    let (mut s, item) = setup();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    s.execute("timeline.setTrack", json!({"track":"A1","locked":true})).unwrap();
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    let history = s.execute("history.list", json!({})).unwrap();
    let params = json!({"item":item,"track":"V1","audioTrack":"A1","sourceIn":ticks(8.0),"duration":ticks(8.0),"time":ticks(40.0)});
    assert!(s.execute("timeline.planPlacement", params.clone()).is_err());
    assert!(s.execute("timeline.place", params).is_err());
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), before);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
}

#[test]
fn contiguous_role_and_source_coordinates_survive_disk_reopen() {
    let (mut s, item) = setup();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    place(&mut s, item, 8.0, 8.0, 40.0);
    place(&mut s, item, 30.0, 6.0, 50.0);
    let before = s.execute("sequence.inspect", json!({})).unwrap();
    let dir = std::env::temp_dir().join(format!("filmcraft-contiguous-reopen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("editing.fcproj");
    s.execute("file.saveAs", json!({"path":path})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":path})).unwrap();
    let q = reopened.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(q["editingTrack"], before["editingTrack"]);
    assert_eq!(q["video"], before["video"]);
    assert_eq!(q["audio"], before["audio"]);
    reopened.execute("playhead.set", json!({"seconds":9.0})).unwrap();
    reopened.execute("sequence.matchFrame", json!({})).unwrap();
    let source = reopened.execute("source.inspect", json!({})).unwrap();
    assert_eq!(source["item"], json!(item));
    assert_eq!(source["playhead"], json!(ticks(31.0)));
    place(&mut reopened, item, 20.0, 3.0, 8.0);
    let q = reopened.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(spans(&q, "video"), vec![(0, ticks(8.0), ticks(8.0)), (ticks(8.0), ticks(3.0), ticks(20.0)), (ticks(11.0), ticks(6.0), ticks(30.0))]);
    std::fs::remove_dir_all(dir).unwrap();
}
