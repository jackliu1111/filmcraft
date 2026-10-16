use crate::Session;
use filmcraft_project::ItemId;
use filmcraft_time::{FrameRate, Tick};
use serde_json::{Value, json};
fn t(seconds: f64) -> i64 {
    Tick::from_seconds_f64(seconds).0
}
fn setup() -> (Session, String) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.edit("Fixture", |p, _| {
        let m = p.item_mut(ItemId(5)).unwrap().as_media_mut().unwrap();
        m.info.duration = Tick(t(100.0));
        m.info.video.as_mut().unwrap().frame_rate = FrameRate::new(24, 1);
        Ok(())
    })
    .unwrap();
    s.execute("file.newSequence", json!({"width":1920,"height":1080,"fps":24,"video":1,"audio":1})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    let range = s.execute("selection.addRange", json!({"item":5,"start":t(8.0),"duration":t(8.0)})).unwrap()["rangeId"].as_str().unwrap().to_string();
    (s, range)
}
fn sequence(s: &mut Session) -> Value {
    s.execute("sequence.inspect", json!({})).unwrap()
}
#[test]
fn selection_drag_capture_places_its_own_linked_range_even_when_selection_changes() {
    let (mut s, id) = setup();
    let snapshot = s.execute("selection.capture", json!({"item":5,"rangeId":id})).unwrap();
    s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":t(7.0),"duration":t(11.0)})).unwrap();
    let before = s.execute("selection.inspect", json!({"item":5})).unwrap();
    s.execute("selection.place", json!({"snapshot":snapshot,"append":true})).unwrap();
    let q = sequence(&mut s);
    assert_eq!(q["video"][0]["items"][0]["start"], 0);
    assert_eq!(q["video"][0]["items"][0]["duration"], t(8.0));
    assert_eq!(q["video"][0]["items"][0]["sourceIn"], t(8.0));
    assert_eq!(q["audio"][0]["items"][0]["duration"], t(8.0));
    assert_eq!(q["audio"][0]["items"][0]["link"], q["video"][0]["items"][0]["link"]);
    assert!(!q["video"][0]["items"][0]["link"].is_null());
    assert_eq!(s.execute("selection.inspect", json!({"item":5})).unwrap(), before);
    s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":t(6.0),"duration":t(13.0)})).unwrap();
    assert_eq!(sequence(&mut s)["video"], q["video"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("selection.inspect", json!({"item":5})).unwrap(), before);
    assert_eq!(sequence(&mut s)["video"], q["video"]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(sequence(&mut s)["duration"], 0);
    assert_eq!(s.execute("selection.inspect", json!({"item":5})).unwrap(), before);
}

#[test]
fn selection_usage_counts_linked_units_allows_overlap_and_follows_undo() {
    let (mut s, id) = setup();
    s.execute("selection.setRange", json!({"item":5,"rangeId":id,"start":t(10.0),"duration":t(10.0)})).unwrap();
    let second = s.execute("selection.addRange", json!({"item":5,"start":t(15.0),"duration":t(10.0)})).unwrap()["rangeId"].clone();
    let first = s.execute("selection.capture", json!({"item":5,"rangeId":id})).unwrap();
    s.execute("selection.place", json!({"snapshot":first,"append":true})).unwrap();
    let used = s.execute("selection.usage", json!({"item":5,"rangeId":id})).unwrap();
    assert_eq!(used["status"], "used");
    assert_eq!(used["uses"], 1);
    assert_eq!(used["covered"], t(10.0));
    let partial = s.execute("selection.usage", json!({"item":5,"rangeId":second})).unwrap();
    assert_eq!(partial["status"], "partial");
    assert_eq!(partial["covered"], t(5.0));
    let second = s.execute("selection.capture", json!({"item":5,"rangeId":second})).unwrap();
    s.execute("selection.place", json!({"snapshot":second,"append":true})).unwrap();
    let q = sequence(&mut s);
    assert_eq!(q["duration"], t(20.0));
    assert_eq!(q["video"][0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(s.execute("selection.usage", json!({"item":5,"rangeId":id})).unwrap()["uses"], 2);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("selection.usage", json!({"item":5,"start":t(15.0),"duration":t(10.0)})).unwrap(), partial);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.execute("selection.usage", json!({"item":5,"rangeId":id})).unwrap()["status"], "unused");
}

#[test]
fn selection_plan_and_ordinary_source_drop_share_insertion_and_one_undo() {
    let (mut s, id) = setup();
    let first = s.execute("selection.capture", json!({"item":5,"rangeId":id})).unwrap();
    s.execute("selection.place", json!({"snapshot":first,"append":true})).unwrap();
    s.execute("timeline.place", json!({"item":5,"track":"V1","audioTrack":"A1","sourceIn":t(30.0),"duration":t(6.0),"time":t(100.0)})).unwrap();
    let inserted = s.execute("selection.addRange", json!({"item":5,"start":t(20.0),"duration":t(3.0)})).unwrap()["rangeId"].clone();
    let snapshot = s.execute("selection.capture", json!({"item":5,"rangeId":inserted})).unwrap();
    let params = json!({"snapshot":snapshot,"time":t(8.1)});
    let before = sequence(&mut s);
    let history = s.execute("history.list", json!({})).unwrap();
    let planned = s.execute("selection.planPlacement", params.clone()).unwrap();
    let native = s
        .execute(
            "timeline.planPlacement",
            json!({"item":5,"track":"V1","audioTrack":"A1","sourceIn":t(20.0),"duration":t(3.0),"time":t(8.1),"insert":true,"video":true,"audio":true}),
        )
        .unwrap();
    assert_eq!(planned, native);
    assert_eq!(planned["time"], t(8.0));
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    s.execute("selection.place", params).unwrap();
    let q = sequence(&mut s);
    for kind in ["video", "audio"] {
        let spans = q[kind][0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["start"].as_i64().unwrap(), c["duration"].as_i64().unwrap(), c["sourceIn"].as_i64().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(spans, vec![(0, t(8.0), t(8.0)), (t(8.0), t(3.0), t(20.0)), (t(11.0), t(6.0), t(30.0))]);
    }
    assert_eq!(s.execute("history.list", json!({})).unwrap()["undo"].as_array().unwrap().len(), history["undo"].as_array().unwrap().len() + 1);
    s.execute("edit.undo", json!({})).unwrap();
    let mut after = sequence(&mut s);
    let mut before = before;
    // Native undo clears ephemeral clip selection; all persisted sequence content must match.
    assert_eq!(after["selection"], json!([]));
    before.as_object_mut().unwrap().remove("selection");
    after.as_object_mut().unwrap().remove("selection");
    assert_eq!(after, before);
}

#[test]
fn selection_invalid_target_and_stale_project_do_not_change_current_work() {
    let (mut s, id) = setup();
    let snapshot = s.execute("selection.capture", json!({"item":5,"rangeId":id})).unwrap();
    let before = sequence(&mut s);
    let history = s.execute("history.list", json!({})).unwrap();
    assert!(s.execute("selection.place", json!({"snapshot":snapshot,"track":"new","append":true})).is_err());
    assert_eq!(sequence(&mut s), before);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    s.execute("clip.audioChannels", json!({"items":[5],"format":"mono"})).unwrap();
    assert!(s.execute("selection.place", json!({"snapshot":snapshot,"append":true})).is_err());
    assert_eq!(sequence(&mut s), before);
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"enabled":false})).unwrap();
    let before = sequence(&mut s);
    assert!(s.execute("selection.place", json!({"snapshot":snapshot,"append":true})).is_err());
    assert_eq!(sequence(&mut s), before);
    s.execute("file.newProject", json!({"name":"Other"})).unwrap();
    let before = s.execute("project.inspect", json!({})).unwrap();
    assert!(s.execute("selection.place", json!({"snapshot":snapshot,"append":true})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
}

#[test]
fn selection_placement_and_source_mapping_survive_disk_open_without_restoring_old_drag() {
    let (mut s, id) = setup();
    let snapshot = s.execute("selection.capture", json!({"item":5,"rangeId":id})).unwrap();
    s.execute("selection.place", json!({"snapshot":snapshot,"append":true})).unwrap();
    let q = sequence(&mut s);
    let usage = s.execute("selection.usage", json!({"item":5,"rangeId":id})).unwrap();
    let path = format!("/private/tmp/filmcraft-selection-placement-{}-{:?}.fcproj", std::process::id(), std::thread::current().id());
    s.execute("file.saveAs", json!({"path":path})).unwrap();
    let mut opened = Session::default();
    opened.execute("file.open", json!({"path":path})).unwrap();
    let reopened = sequence(&mut opened);
    assert_eq!(reopened["video"], q["video"]);
    assert_eq!(reopened["audio"], q["audio"]);
    assert_eq!(opened.execute("selection.usage", json!({"item":5,"rangeId":id})).unwrap(), usage);
    assert!(opened.execute("selection.place", json!({"snapshot":snapshot,"append":true})).is_err());
    std::fs::remove_file(path).unwrap();
}
