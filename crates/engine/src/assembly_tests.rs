use crate::Session;
use filmcraft_time::Tick;
use serde_json::{Value, json};

fn t(s: f64) -> i64 {
    Tick::from_seconds_f64(s).0
}
fn folder() -> String {
    format!("/private/tmp/filmcraft-assembly-{}-{:?}", std::process::id(), std::thread::current().id())
}
fn setup() -> Session {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    s.edit("Fixture", |p, _| {
        let media = p.item_mut(filmcraft_project::ItemId(5)).unwrap().as_media_mut().unwrap();
        media.info.duration = Tick(t(100.0));
        media.info.video.as_mut().unwrap().frame_rate = filmcraft_time::FrameRate::new(24, 1);
        Ok(())
    })
    .unwrap();
    s.execute("file.newSequence", json!({"name":"人工剪辑","width":1920,"height":1080,"fps":24,"video":1,"audio":1})).unwrap();
    s.execute("timeline.place", json!({"item":5,"track":"V1","audioTrack":"A1","time":0,"sourceIn":t(1.0),"duration":t(2.0)})).unwrap();
    s
}
fn add(s: &mut Session, start: f64, end: f64, category: &str) -> Value {
    s.execute("selection.addRange", json!({"item":5,"start":t(start),"duration":t(end-start),"category":category})).unwrap()
}

#[test]
fn assembly_unions_current_ranges_and_opens_linked_copy_without_changing_manual_work() {
    let mut s = setup();
    let first = add(&mut s, 10.0, 20.0, "gunshot");
    let second = add(&mut s, 15.0, 25.0, "gunshot");
    add(&mut s, 40.0, 44.0, "voice");
    let manual = s.execute("sequence.inspect", json!({})).unwrap();
    let root = folder();
    let assembled = s.execute("selection.assemble", json!({"item":5,"directory":root})).unwrap();
    assert_eq!(assembled["status"], "assembled");
    assert_eq!(assembled["clipCount"], 1);
    assert_eq!(assembled["duration"], t(15.0));
    assert_eq!(assembled["segments"][0]["sourceIn"], t(10.0));
    assert_eq!(assembled["segments"][0]["rangeIds"], json!([first["rangeId"], second["rangeId"]]));
    let q = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(q["duration"], t(15.0));
    assert!(!q["editingTrack"].is_null());
    assert_eq!(q["video"][0]["items"][0]["sourceIn"], t(10.0));
    assert_eq!(q["audio"][0]["items"][0]["link"], q["video"][0]["items"][0]["link"]);
    assert_eq!(s.execute("sequence.inspect", json!({"item":manual["id"]})).unwrap()["video"], manual["video"]);
    let mut previous = Session::default();
    previous.execute("file.open", json!({"path":assembled["preservedProject"]})).unwrap();
    assert_eq!(previous.execute("sequence.inspect", json!({})).unwrap()["video"], manual["video"]);
    assert_eq!(s.execute("project.inspect", json!({})).unwrap()["path"], assembled["editProject"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn assembly_persists_relationship_and_protects_original_across_copy_edits_and_reassembly() {
    let mut s = setup();
    let range = add(&mut s, 10.0, 25.0, "gunshot");
    let root = folder();
    let first = s.execute("selection.assemble", json!({"item":5,"directory":root})).unwrap();
    let relation = s.execute("selection.assembly", json!({})).unwrap();
    assert_eq!(relation["role"], "editingCopy");
    assert_eq!(relation["assembly"]["originalProject"], first["originalProject"]);
    assert_eq!(relation["assembly"]["ranges"][0]["rangeId"], range["rangeId"]);
    let clip = s.execute("sequence.inspect", json!({})).unwrap()["video"][0]["items"][0]["clip"].clone();
    s.execute("timeline.trim", json!({"clip":clip,"edge":"out","delta":t(-1.0)})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    let mut baseline = Session::default();
    baseline.execute("file.open", json!({"path":first["originalProject"]})).unwrap();
    assert_eq!(baseline.execute("selection.assembly", json!({})).unwrap()["role"], "original");
    assert_eq!(baseline.execute("sequence.inspect", json!({})).unwrap()["duration"], t(15.0));
    assert!(baseline.execute("file.save", json!({})).is_err());
    assert!(s.execute("file.saveCopy", json!({"path":first["originalProject"]})).is_err());
    let mut copy = Session::default();
    copy.execute("file.open", json!({"path":first["editProject"]})).unwrap();
    assert_eq!(copy.execute("sequence.inspect", json!({})).unwrap()["duration"], t(14.0));
    copy.execute("selection.setRange", json!({"item":5,"rangeId":range["rangeId"],"start":t(8.0),"duration":t(8.0)})).unwrap();
    let second = copy.execute("selection.assemble", json!({"item":5,"directory":root})).unwrap();
    assert_ne!(second["originalProject"], first["originalProject"]);
    let relation = copy.execute("selection.assembly", json!({})).unwrap();
    assert_eq!(relation["assemblies"].as_array().unwrap().len(), 2);
    assert_eq!(relation["assembly"]["parentProject"], first["editProject"]);
    assert_eq!(relation["assembly"]["ranges"][0]["modified"], true);
    assert_eq!(second["duration"], t(8.0));
    baseline.execute("file.open", json!({"path":first["originalProject"]})).unwrap();
    assert_eq!(baseline.execute("sequence.inspect", json!({})).unwrap()["duration"], t(15.0));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn assembly_uses_adjusted_and_default_categories_without_reanalysis_and_rejects_changed_source() {
    let mut s = setup();
    let root = folder();
    let path = format!("{root}.mp4");
    std::fs::write(&path, b"analysis source fixture").unwrap();
    s.edit("Fixture", |p, _| {
        p.item_mut(filmcraft_project::ItemId(5)).unwrap().as_media_mut().unwrap().media = filmcraft_project::MediaRef::File { path: path.clone() };
        Ok(())
    })
    .unwrap();
    let stat = std::fs::metadata(&path).unwrap();
    let guns = json!([{"event_id":"g1","start_s":10.0,"end_s":12.0},{"event_id":"g2","start_s":30.0,"end_s":31.0}]);
    let result = json!({"schema_version":1,"time_unit":"seconds","result_id":"assembly-current", "media":{
        "source":{"path":path,"size_bytes":stat.len(),"mtime_ns":stat.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64},
        "duration_s":100.0,"video":{"start_time_s":0.0},"selected_audio":{"stream_index":1},"audio_tracks":[{"stream_index":1,"sample_rate":48000,"channels":2}]},
        "events":guns,"candidates":[{"start_s":5.0,"end_s":17.0},{"start_s":25.0,"end_s":36.0}],
        "categories":{"gunshot":{"status":"completed","events":guns},"voice":{"status":"completed","events":[{"event_id":"v1","start_s":70.0,"end_s":72.0}]}}});
    let token = s.execute("recognition.begin", json!({"item":5,"audioStream":1})).unwrap();
    s.execute("recognition.attach", json!({"token":token["token"],"result":result})).unwrap();
    let raw = s.execute("recognition.inspect", json!({"item":5})).unwrap();
    let ranges = s.execute("selection.inspect", json!({"item":5})).unwrap();
    s.execute("selection.setRange", json!({"item":5,"rangeId":ranges["ranges"][0]["rangeId"],"start":t(8.0),"duration":t(8.0)})).unwrap();
    let current = s.execute("selection.inspect", json!({"item":5})).unwrap()["ranges"].clone();
    let assembled = s.execute("selection.assemble", json!({"item":5,"categories":["gunshot","voice"],"directory":root})).unwrap();
    assert_eq!(assembled["duration"], t(23.0));
    let spans = assembled["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["sourceIn"].as_i64().unwrap(), v["duration"].as_i64().unwrap(), v["start"].as_i64().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(spans, vec![(t(8.0), t(8.0), 0), (t(25.0), t(11.0), t(8.0)), (t(69.0), t(4.0), t(19.0))]);
    assert_eq!(s.execute("recognition.inspect", json!({"item":5})).unwrap(), raw);
    assert_eq!(s.execute("selection.inspect", json!({"item":5})).unwrap()["ranges"], current);
    std::fs::write(&path, b"source was replaced after analysis").unwrap();
    let before = s.execute("project.inspect", json!({})).unwrap();
    let edits = s.execute("sequence.inspect", json!({})).unwrap();
    assert!(s.execute("selection.assemble", json!({"item":5,"directory":root})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), edits);
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn assembly_empty_and_failed_save_preserve_project_path_unsaved_work_and_history() {
    let mut s = setup();
    let root = folder();
    let project = s.execute("project.inspect", json!({})).unwrap();
    let history = s.execute("history.list", json!({})).unwrap();
    assert_eq!(s.execute("selection.assemble", json!({"item":5,"directory":root})).unwrap()["status"], "noRanges");
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), project);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    assert!(!std::path::Path::new(&root).exists());
    add(&mut s, 10.0, 20.0, "gunshot");
    let manual = format!("{root}.fcproj");
    s.execute("file.saveAs", json!({"path":manual})).unwrap();
    add(&mut s, 30.0, 35.0, "gunshot"); // Deliberately leave work unsaved when publication fails.
    struct FailsCopy;
    impl crate::Services for FailsCopy {
        fn read_file(&self, path: &str) -> std::io::Result<Vec<u8>> {
            std::fs::read(path)
        }
        fn write_file(&self, path: &str, data: &[u8]) -> std::io::Result<()> {
            if path.ends_with("/edit-copy.fcproj") {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture refuses editing-copy publication"));
            }
            crate::Services::write_file(&crate::FsServices, path, data)
        }
    }
    s.services = std::sync::Arc::new(FailsCopy);
    let before = s.execute("project.inspect", json!({})).unwrap();
    let edits = s.execute("sequence.inspect", json!({})).unwrap();
    let ranges = s.execute("selection.inspect", json!({"item":5})).unwrap();
    let history = s.execute("history.list", json!({})).unwrap();
    assert!(s.execute("selection.assemble", json!({"item":5,"directory":root})).is_err());
    assert_eq!(s.execute("project.inspect", json!({})).unwrap(), before);
    assert_eq!(s.execute("sequence.inspect", json!({})).unwrap(), edits);
    assert_eq!(s.execute("selection.inspect", json!({"item":5})).unwrap(), ranges);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    let mut disk = Session::default();
    disk.execute("file.open", json!({"path":manual})).unwrap();
    assert_eq!(disk.execute("selection.inspect", json!({"item":5})).unwrap()["ranges"].as_array().unwrap().len(), 1);
    std::fs::remove_dir(root).unwrap();
    std::fs::remove_file(manual).unwrap();
}

#[test]
fn assembly_retains_every_native_audio_stream_as_one_editing_unit() {
    let mut s = setup();
    s.edit("Fixture", |p, _| {
        let m = p.item_mut(filmcraft_project::ItemId(5)).unwrap().as_media_mut().unwrap();
        let mut audio = m.info.audio_streams[0].clone();
        audio.channels = 1;
        m.info.audio_streams.push(audio);
        Ok(())
    })
    .unwrap();
    add(&mut s, 10.0, 25.0, "gunshot");
    let root = folder();
    s.execute("selection.assemble", json!({"item":5,"directory":root})).unwrap();
    let q = s.execute("sequence.inspect", json!({})).unwrap();
    assert_eq!(q["editingTrack"]["audio"].as_array().unwrap().len(), 2);
    let video = &q["video"][0]["items"][0];
    for (index, audio) in q["audio"].as_array().unwrap().iter().enumerate() {
        let sound = &audio["items"][0];
        assert_eq!(sound["audioStream"], index);
        assert_eq!(sound["sourceIn"], t(10.0));
        assert_eq!(sound["duration"], t(15.0));
        assert_eq!(sound["link"], video["link"]);
    }
    std::fs::remove_dir_all(root).unwrap();
}
