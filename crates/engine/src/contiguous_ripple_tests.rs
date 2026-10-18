use crate::Session;
use filmcraft_time::Tick;
use serde_json::{Value, json};

fn tick(seconds: f64) -> i64 {
    Tick::from_seconds_f64(seconds).0
}
fn view(s: &mut Session) -> Value {
    s.execute("sequence.inspect", json!({})).unwrap()
}
fn clips(q: &Value, kind: &str) -> Vec<(u64, i64, i64, i64)> {
    q[kind][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["clip"].as_u64().unwrap(), c["start"].as_i64().unwrap(), c["duration"].as_i64().unwrap(), c["sourceIn"].as_i64().unwrap()))
        .collect()
}
fn three() -> Session {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name":"Contiguous edits","fps":24,"video":2,"audio":2})).unwrap();
    let item = s.execute("file.newOfflineFile", json!({"name":"Original","seconds":60,"fps":24,"video":true,"audio":true})).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item":item,"track":"V2","audioTrack":"A2","time":tick(4.0),"duration":tick(20.0)})).unwrap();
    s.execute("sequence.setEditingTrack", json!({"track":"V1","audioTrack":"A1"})).unwrap();
    for (start, duration) in [(8.0, 8.0), (20.0, 3.0), (30.0, 6.0)] {
        s.execute("timeline.place", json!({"item":item,"track":"V1","audioTrack":"A1","time":tick(60.0),"sourceIn":tick(start),"duration":tick(duration)}))
            .unwrap();
    }
    s.execute("sequence.linkedSelection", json!({"on":false})).unwrap();
    s
}
#[test]
fn deleting_a_target_clip_closes_its_linked_unit_without_moving_native_tracks() {
    let mut s = three();
    let before = view(&mut s);
    let middle = before["video"][0]["items"][1]["clip"].as_u64().unwrap();
    s.execute("timeline.select", json!({"clips":[middle],"linked":false})).unwrap();
    s.execute("edit.clear", json!({})).unwrap();
    let after = view(&mut s);
    let old = clips(&before, "video");
    assert_eq!(clips(&after, "video"), vec![old[0], (old[2].0, tick(8.0), tick(6.0), tick(30.0))]);
    assert_eq!(after["video"][1], before["video"][1]);
    assert_eq!(after["audio"][1], before["audio"][1]);
    for (v, a) in after["video"][0]["items"].as_array().unwrap().iter().zip(after["audio"][0]["items"].as_array().unwrap()) {
        assert_eq!(v["start"], a["start"]);
        assert_eq!(v["duration"], a["duration"]);
        assert_eq!(v["link"], a["link"]);
    }
    s.execute("edit.undo", json!({})).unwrap();
    let undo = view(&mut s);
    assert_eq!(undo["video"], before["video"]);
    assert_eq!(undo["audio"], before["audio"]);
}

#[test]
fn target_regular_edge_drag_trims_the_selected_unit_and_closes_the_tail() {
    let mut s = three();
    let q = view(&mut s);
    let middle = q["video"][0]["items"][1]["clip"].as_u64().unwrap();
    s.execute("edit.clear", json!({"clips":[middle]})).unwrap();
    let before = view(&mut s);
    let first = before["video"][0]["items"][0]["clip"].as_u64().unwrap();
    let selection = s.execute("timeline.select", json!({"clips":[first],"linked":false})).unwrap();
    assert_eq!(selection["selection"].as_array().unwrap().len(), 2, "the target always selects an associated unit");
    let result = s.execute("timeline.trim", json!({"clip":first,"edge":"out","mode":"regular","delta":tick(-2.0)})).unwrap();
    assert_eq!(result["delta"], json!(tick(-2.0)));
    let after = view(&mut s);
    let old = clips(&before, "video");
    assert_eq!(clips(&after, "video"), vec![(old[0].0, 0, tick(6.0), tick(8.0)), (old[1].0, tick(6.0), tick(6.0), tick(30.0))]);
    assert_eq!(after["video"][1], before["video"][1]);
    assert_eq!(after["audio"][1], before["audio"][1]);
    for (v, a) in after["video"][0]["items"].as_array().unwrap().iter().zip(after["audio"][0]["items"].as_array().unwrap()) {
        assert_eq!(v["start"], a["start"]);
        assert_eq!(v["duration"], a["duration"]);
        assert_eq!(v["sourceIn"], a["sourceIn"]);
    }
    s.execute("edit.undo", json!({})).unwrap();
    let undo = view(&mut s);
    assert_eq!(undo["video"], before["video"]);
    assert_eq!(undo["audio"], before["audio"]);
}

#[test]
fn reordering_a_target_unit_closes_its_old_position_and_preserves_ids_and_sources() {
    let mut s = three();
    let before = view(&mut s);
    let old = clips(&before, "video");
    for (clip, at, expected) in [
        (old[0].0, 60.0, vec![(old[1].0, 0, tick(3.0), tick(20.0)), (old[2].0, tick(3.0), tick(6.0), tick(30.0)), (old[0].0, tick(9.0), tick(8.0), tick(8.0))]),
        (old[2].0, 0.0, vec![(old[2].0, 0, tick(6.0), tick(30.0)), (old[0].0, tick(6.0), tick(8.0), tick(8.0)), (old[1].0, tick(14.0), tick(3.0), tick(20.0))]),
    ] {
        s.execute("timeline.move", json!({"moves":[{"clip":clip,"track":"V1","time":tick(at)}],"linked":false})).unwrap();
        let after = view(&mut s);
        assert_eq!(clips(&after, "video"), expected);
        assert_eq!(after["video"][1], before["video"][1]);
        assert_eq!(after["audio"][1], before["audio"][1]);
        for (v, a) in after["video"][0]["items"].as_array().unwrap().iter().zip(after["audio"][0]["items"].as_array().unwrap()) {
            assert_eq!(v["start"], a["start"]);
            assert_eq!(v["sourceIn"], a["sourceIn"]);
            assert_eq!(v["link"], a["link"]);
        }
        s.execute("edit.undo", json!({})).unwrap();
        let undo = view(&mut s);
        assert_eq!(undo["video"], before["video"]);
        assert_eq!(undo["audio"], before["audio"]);
    }
}

#[test]
fn edit_preview_is_pure_and_matches_extension_and_reorder_commits() {
    let mut s = three();
    let before = view(&mut s);
    let first = clips(&before, "video")[0].0;
    for (action, command, params) in [
        ("trim", "timeline.trim", json!({"clip":first,"edge":"out","mode":"regular","delta":tick(2.0)})),
        ("move", "timeline.move", json!({"moves":[{"clip":first,"track":"V1","time":tick(60.0)}],"linked":false})),
    ] {
        let history = s.execute("history.list", json!({})).unwrap();
        let mut request = params.clone();
        request["action"] = json!(action);
        let plan = s.execute("timeline.planEdit", request).unwrap();
        assert_eq!(plan["handled"], json!(true));
        assert_eq!(view(&mut s), before);
        assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
        s.execute(command, params).unwrap();
        let after = view(&mut s);
        for planned in plan["clips"].as_array().unwrap() {
            let actual = after["video"]
                .as_array()
                .unwrap()
                .iter()
                .chain(after["audio"].as_array().unwrap())
                .flat_map(|t| t["items"].as_array().unwrap())
                .find(|c| c["clip"] == planned["clip"])
                .unwrap();
            for key in ["start", "duration", "sourceIn", "item", "link"] {
                assert_eq!(actual[key], planned[key], "{action} {key}");
            }
        }
        if action == "trim" {
            assert_eq!(clips(&after, "video")[0].2, tick(10.0));
            assert_eq!(clips(&after, "video")[1].1, tick(10.0));
        }
        s.execute("edit.undo", json!({})).unwrap();
    }
}

#[test]
fn locked_retained_sound_refuses_every_partial_target_edit_and_its_preview() {
    let mut s = three();
    let first = clips(&view(&mut s), "video")[0].0;
    s.execute("timeline.setTrack", json!({"track":"A1","locked":true})).unwrap();
    let before = view(&mut s);
    let history = s.execute("history.list", json!({})).unwrap();
    for (command, params) in [
        ("edit.clear", json!({"clips":[first]})),
        ("edit.rippleDelete", json!({"clips":[first]})),
        ("timeline.trim", json!({"clip":first,"edge":"out","delta":tick(-2.0)})),
        ("timeline.move", json!({"moves":[{"clip":first,"track":"V1","time":tick(60.0)}],"linked":false})),
        ("timeline.planEdit", json!({"action":"trim","clip":first,"edge":"out","delta":tick(-2.0)})),
        ("timeline.planEdit", json!({"action":"move","moves":[{"clip":first,"track":"V1","time":tick(60.0)}]})),
        ("timeline.roll", json!({"left":first,"right":before["video"][0]["items"][1]["clip"],"delta":tick(1.0),"followLinks":false})),
    ] {
        assert!(s.execute(command, params).is_err(), "{command}");
        assert_eq!(view(&mut s), before);
        assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    }
    s.execute("timeline.setTrack", json!({"track":"A1","locked":false})).unwrap();
    s.execute("timeline.roll", json!({"left":first,"right":before["video"][0]["items"][1]["clip"],"delta":tick(1.0),"followLinks":false})).unwrap();
    let rolled = view(&mut s);
    for (v, a) in rolled["video"][0]["items"].as_array().unwrap().iter().zip(rolled["audio"][0]["items"].as_array().unwrap()) {
        assert_eq!(v["start"], a["start"]);
        assert_eq!(v["duration"], a["duration"]);
        assert_eq!(v["sourceIn"], a["sourceIn"]);
    }
}

#[test]
fn target_edges_clamp_to_source_bounds_and_first_last_all_delete_without_gaps() {
    let mut s = three();
    let before = view(&mut s);
    let old = clips(&before, "video");
    let first = old[0].0;
    let result = s.execute("timeline.trim", json!({"clip":first,"edge":"out","delta":tick(100.0)})).unwrap();
    assert_eq!(result["delta"], json!(tick(44.0)));
    let q = view(&mut s);
    assert_eq!(q["video"][0]["items"][0]["sourceOut"], json!(tick(60.0)));
    assert_eq!(clips(&q, "video")[1].1, tick(52.0));
    s.execute("edit.undo", json!({})).unwrap();
    let result = s.execute("timeline.trim", json!({"clip":first,"edge":"in","delta":tick(-100.0)})).unwrap();
    assert_eq!(result["delta"], json!(tick(-8.0)));
    let q = view(&mut s);
    assert_eq!(clips(&q, "video")[0], (first, 0, tick(16.0), 0));
    assert_eq!(clips(&q, "video")[1].1, tick(16.0));
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("timeline.trim", json!({"clip":first,"edge":"in","delta":tick(100.0)})).unwrap();
    let q = view(&mut s);
    assert_eq!(clips(&q, "video")[0].2, tick(1.0 / 24.0));
    assert_eq!(clips(&q, "video")[1].1, tick(1.0 / 24.0));
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.rippleDelete", json!({"clips":[old[0].0]})).unwrap();
    let q = view(&mut s);
    assert_eq!(clips(&q, "video")[0], (old[1].0, 0, tick(3.0), tick(20.0)));
    assert_eq!(clips(&q, "video")[1].1, tick(3.0));
    s.execute("edit.clear", json!({"clips":[old[2].0]})).unwrap();
    let q = view(&mut s);
    assert_eq!(clips(&q, "video").len(), 1);
    s.execute("edit.clear", json!({"clips":[old[1].0]})).unwrap();
    let q = view(&mut s);
    assert!(clips(&q, "video").is_empty());
    assert!(clips(&q, "audio").is_empty());
    assert_eq!(q["video"][1], before["video"][1]);
    assert_eq!(q["audio"][1], before["audio"][1]);
}

#[test]
fn explicit_copy_packs_non_adjacent_units_without_copying_the_gaps_between_them() {
    let mut s = three();
    let before = view(&mut s);
    let old = clips(&before, "video");
    s.execute(
        "timeline.move",
        json!({"moves":[{"clip":old[0].0,"track":"V1","time":tick(60.0)},{"clip":old[2].0,"track":"V1","time":tick(71.0)}],"copy":true,"linked":false}),
    )
    .unwrap();
    let q = view(&mut s);
    let actual = clips(&q, "video");
    assert_eq!(
        actual.iter().map(|c| (c.1, c.2, c.3)).collect::<Vec<_>>(),
        vec![
            (0, tick(8.0), tick(8.0)),
            (tick(8.0), tick(3.0), tick(20.0)),
            (tick(11.0), tick(6.0), tick(30.0)),
            (tick(17.0), tick(8.0), tick(8.0)),
            (tick(25.0), tick(6.0), tick(30.0))
        ]
    );
    assert!(actual[3..].iter().all(|c| !old.iter().any(|old| old.0 == c.0)));
    assert_eq!(q["audio"][0]["items"].as_array().unwrap().len(), 5);
    assert_eq!(q["audio"][1], before["audio"][1]);
    s.execute("edit.undo", json!({})).unwrap();
    let undo = view(&mut s);
    assert_eq!(undo["video"], before["video"]);
    assert_eq!(undo["audio"], before["audio"]);
}

#[test]
fn native_trim_shortcuts_apply_once_per_target_unit_and_leave_native_tracks_alone() {
    let mut s = three();
    let before = view(&mut s);
    let first = clips(&before, "video")[0].0;
    s.execute("timeline.select", json!({"clips":[first],"linked":false})).unwrap();
    s.execute("playhead.set", json!({"seconds":2.0})).unwrap();
    s.execute("trim.ripplePrevious", json!({})).unwrap();
    let q = view(&mut s);
    assert_eq!(clips(&q, "video")[0], (first, 0, tick(6.0), tick(10.0)));
    assert_eq!(q["video"][1], before["video"][1]);
    assert_eq!(q["audio"][1], before["audio"][1]);
    s.execute("edit.undo", json!({})).unwrap();
    let audio = before["audio"][0]["items"][0]["clip"].as_u64().unwrap();
    s.execute("trim.selectEditPoint", json!({"clip":first,"edge":"out","kind":"trim"})).unwrap();
    s.execute("trim.selectEditPoint", json!({"clip":audio,"edge":"out","kind":"trim","add":true})).unwrap();
    s.execute("trim.nudge", json!({"frames":-24})).unwrap();
    let q = view(&mut s);
    assert_eq!(clips(&q, "video")[0].2, tick(7.0), "two selected channel edges are one editing unit");
    assert_eq!(clips(&q, "video")[1].1, tick(7.0));
    assert_eq!(q["audio"][1], before["audio"][1]);
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("trim.selectEditPoint", json!({"clip":first,"edge":"out","kind":"roll"})).unwrap();
    s.execute("trim.selectEditPoint", json!({"clip":audio,"edge":"out","kind":"roll","add":true})).unwrap();
    s.execute("trim.nudge", json!({"frames":24})).unwrap();
    let q = view(&mut s);
    assert_eq!(clips(&q, "video")[0].2, tick(9.0), "the mirrored rolling edit is applied once");
}

#[test]
fn reordered_trimmed_units_reopen_with_their_identity_and_keep_the_editing_policy() {
    let mut s = three();
    let old = clips(&view(&mut s), "video");
    s.execute("timeline.move", json!({"moves":[{"clip":old[0].0,"track":"V1","time":tick(60.0)}]})).unwrap();
    s.execute("timeline.trim", json!({"clip":old[1].0,"edge":"out","delta":tick(-1.0)})).unwrap();
    let before = view(&mut s);
    let dir = std::env::temp_dir().join(format!("filmcraft-edited-reopen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("edited.fcproj");
    s.execute("file.saveAs", json!({"path":path})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    let mut reopened = Session::default();
    reopened.execute("file.open", json!({"path":path})).unwrap();
    let q = view(&mut reopened);
    assert_eq!(q["video"], before["video"]);
    assert_eq!(q["audio"], before["audio"]);
    assert_eq!(q["editingTrack"], before["editingTrack"]);
    reopened.execute("edit.clear", json!({"clips":[old[2].0]})).unwrap();
    let q = view(&mut reopened);
    assert_eq!(clips(&q, "video"), vec![(old[1].0, 0, tick(2.0), tick(20.0)), (old[0].0, tick(2.0), tick(8.0), tick(8.0))]);
    std::fs::remove_dir_all(dir).unwrap();
}
