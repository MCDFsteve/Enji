use super::*;
use std::{fs, time::{Duration, Instant}};

fn declaration(id: &str, permissions: &[&str]) -> Value {
    json!({ "id": id, "name": "SDK test", "version": "1", "permissions": permissions,
        "actions": [{ "id": "inspect", "label": "Inspect", "context": "track" }] })
}

fn run(plugins: &mut Plugins, pack: Value, script: &str) {
    let folder = std::env::temp_dir().join(format!("enji-sdk-{:032x}", rand::random::<u128>()));
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("main.js"), script).unwrap();
    plugins.run(Pack::from_json(&pack).unwrap(), &folder).unwrap();
    fs::remove_dir_all(folder).unwrap();
}

fn drain(plugins: &mut Plugins) -> Vec<(String, Call)> {
    let end = Instant::now() + Duration::from_secs(5);
    let mut calls = Vec::new();
    loop {
        calls.extend(plugins.calls());
        if plugins.running.iter().all(|r| r.pending.is_empty()) { return calls; }
        assert!(Instant::now() < end, "plugin did not finish the queued events");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reject_invalid_declarations_before_any_script_runs() {
    let mut p = declaration("a", &[]);
    for v in [
        json!({"id":"s","label":"S","type":"boolean","default":1}),
        json!({"id":"s","label":"S","type":"boolean","default":true,"min":0}),
        json!({"id":"s","label":"S","type":"number","min":2,"max":1,"default":1}),
        json!({"id":"s","label":"S","type":"number","min":1,"max":3,"default":4}),
        json!({"id":"s","label":"S","type":"enum","options":["x","x"],"default":"x"}),
        json!({"id":"s","label":"S","type":"enum","options":["x"],"default":"y"}),
    ] {
        p["settings"] = json!([v]);
        assert!(Pack::from_json(&p).is_err());
    }
    p["settings"] = json!([]);
    p["actions"] = json!([p["actions"][0], p["actions"][0]]);
    assert!(Pack::from_json(&p).is_err());
    p["actions"] = json!([]);
    for bad in [json!(null), json!(2), json!("0.3"), json!("^0.3.1"), json!("00.3.1")] {
        p["minHostVersion"] = bad;
        assert!(Pack::from_json(&p).is_err());
    }
    p["minHostVersion"] = json!("999.0.0");
    assert!(Pack::from_json(&p).unwrap().check_compatibility().is_err());
    p["minHostVersion"] = json!("0.3.19");
    assert!(Pack::from_json(&p).unwrap().check_compatibility().is_ok());
}

#[test]
fn projections_separate_read_permissions_and_never_copy_a_whole_queue_or_timeline() {
    let pack = |permissions: &[&str]| Pack::from_json(&declaration("sdk.read", permissions)).unwrap();
    let source = json!({ "song": {"id":1,"trackKey":"ref","file":"private","sourceKey":"private"}, "playing": true,
        "loudness": {"sourceKey":"file-hash"}, "queue": [1,2], "lyrics": ["line"], "storage": {"other":1} });
    let plain = bridge::snapshot(&pack(&["audio", "queue", "lyrics"]), &source);
    assert_eq!(plain, json!({ "playing":true, "song":{"id":1,"trackKey":"ref"} }));
    for grant in ["audio_gain", "loudness"] {
        let p = pack(&[grant]);
        assert_eq!(bridge::snapshot(&p, &source)["loudness"], source["loudness"]);
        assert!(bridge::broadcast(&p, "loudness", &source["loudness"]).is_some());
        for event in ["audio", "lyric", "lyricsChanged", "queueChanged", "queuePage", "action", "settingChanged"] {
            assert!(bridge::broadcast(&p, event, &Value::Null).is_none());
        }
    }
    let tricky = json!({"text":"A\"\\\n\u{2028}\u{2029}", "__proto__":"just a key"});
    let json_text: String = serde_json::from_str(&bridge::json_string(&tricky)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&json_text).unwrap(), tricky);
}

#[test]
fn only_the_declaring_plugin_receives_the_clicked_track_even_behind_meter_events() {
    let mut plugins = Plugins::default();
    for id in ["sdk.a", "sdk.b"] {
        run(&mut plugins, declaration(id, &["audio"]), "enji.on('action', a => ui.toast(a.track.trackKey + '/' + enji.song.trackKey));");
    }
    let snapshot = json!({ "song": {"id":1,"trackKey":"playing"}, "playbackToken":"p" });
    let clicked = json!({"id":2,"trackKey":"clicked"});
    plugins.send("action", &clicked, &snapshot);
    assert!(plugins.dispatch_action("sdk.a", "other", "track", &clicked, &snapshot).is_err());
    assert!(plugins.dispatch_action("sdk.a", "inspect", "player", &clicked, &snapshot).is_err());
    assert!(plugins.dispatch_action("sdk.a", "inspect", "track", &Value::Null, &snapshot).is_err());
    plugins.dispatch_action("sdk.b", "inspect", "track", &clicked, &snapshot).unwrap();
    for _ in 0..64 { plugins.send("audio", &json!({"beat":0}), &snapshot); }
    assert_eq!(drain(&mut plugins), vec![("sdk.b".into(), Call::Toast("clicked/playing".into()))]);
}

#[test]
fn page_responses_match_the_requester_instance_kind_and_playback() {
    let mut plugins = Plugins::default();
    let script = "enji.on('song', () => { queue.requestPage(0, 1); lyrics.requestTimeline(); });\n\
        enji.on('queuePage', r => ui.toast(r.page.items[0].trackKey));\n\
        enji.on('lyricsTimeline', r => ui.toast(r.error));";
    run(&mut plugins, declaration("sdk.reader", &["queue", "lyrics"]), script);
    run(&mut plugins, declaration("sdk.other", &["queue", "lyrics"]), "enji.on('queuePage', () => ui.toast('wrong reader'))");
    let snapshot = json!({ "playbackToken":"old", "song":{"trackKey":"old"} });
    plugins.send("song", &snapshot["song"], &snapshot);
    let calls = drain(&mut plugins);
    let [(a, Call::QueuePage { request_id: queue, offset: 0, limit: 1 }),
         (b, Call::LyricsTimeline { request_id: lyrics, playback_token, offset: 0, limit: 50 })] = calls.as_slice() else { panic!("{calls:?}") };
    assert!(a == "sdk.reader" && b == a && playback_token == "old" && lyrics != queue);
    let page = json!({"offset":0,"total":1,"currentIndex":0,"revision":1,"playbackToken":"old","items":[{"id":2,"trackKey":"next"}]});
    plugins.reply_page("sdk.other", "queuePage", queue, Ok(page.clone()), &snapshot);
    plugins.reply_page(a, "lyricsTimeline", queue, Err("wrong_kind"), &snapshot);
    plugins.reply_page(a, "queuePage", queue, Ok(page.clone()), &snapshot);
    plugins.reply_page(a, "queuePage", queue, Err("duplicate"), &snapshot);
    plugins.reply_page(a, "lyricsTimeline", lyrics, Ok(page), &json!({"playbackToken":"new"}));
    assert_eq!(drain(&mut plugins), vec![(a.clone(), Call::Toast("next".into())), (a.clone(), Call::Toast("stale_playback".into()))]);
}

#[test]
fn changing_settings_before_collecting_calls_only_invalidates_old_gain_suggestions() {
    let mut plugins = Plugins::default();
    let mut pack = declaration("sdk.revision", &["audio_gain"]);
    pack["settings"] = json!([{"id":"gain","label":"Gain","type":"number","min":-24,"max":0,"default":-4}]);
    run(&mut plugins, pack, "enji.on('song', () => { audio.suggestTrackGain('source', -4); ui.toast('heard'); });\n\
        enji.on('settingChanged', () => audio.suggestTrackGain('source', settings.get('gain')));");
    plugins.send("song", &json!({"trackKey":"song"}), &json!({"playbackToken":"one"}));
    // Change settings without polling either evaluation; no timing or disk-settings dependency.
    plugins.running[0].setting_changed("gain", json!(-9));
    assert_eq!(drain(&mut plugins), vec![
        ("sdk.revision".into(), Call::Toast("heard".into())),
        ("sdk.revision".into(), Call::SuggestTrackGain { source_key: "source".into(), gain_db: -9.0 }),
    ]);
}

#[test]
fn sdk_calls_and_host_checks_keep_write_permissions_and_tokens_distinct() {
    let mut plugins = Plugins::default();
    let script = "enji.on('playback', () => {playback.seekTo(12); audio.suggestTrackGain('k', -8);\n\
        ui.toast([enji.apiVersion, enji.supports('audio.trackGain'), plugin.hasPermission('audio_gain'), audio.suggestTrackGain('k', 13)].join('/'));});";
    run(&mut plugins, declaration("sdk.gain", &["audio_gain"]), script);
    run(&mut plugins, declaration("sdk.control", &["playback_control"]), script);
    assert!(!plugins.wants_audio());
    let snapshot = json!({"playbackToken":"8388292912345678901"});
    plugins.send("playback", &snapshot, &snapshot);
    let calls = drain(&mut plugins);
    assert!(calls.contains(&("sdk.gain".into(), Call::SuggestTrackGain { source_key:"k".into(), gain_db:-8.0 })));
    assert!(calls.contains(&("sdk.gain".into(), Call::Toast("2/true/true/false".into()))));
    let control = Call::PlaybackControl { operation:"seek".into(), playback_token:"8388292912345678901".into(), seconds:Some(12.0) };
    assert!(calls.contains(&("sdk.control".into(), control.clone())));
    assert!(calls.contains(&("sdk.control".into(), Call::Toast("2/true/false/false".into()))));
    assert_eq!(calls.len(), 4);
    assert!(!plugins.running[0].accept(&control, snapshot["playbackToken"].as_str()));
    assert!(!plugins.running[1].accept(&control, Some("later")));
    assert!(!plugins.running[1].accept(&Call::SuggestTrackGain {source_key:"k".into(),gain_db:1.0}, Some("later")));
    for args in [json!(["suggestTrackGain", ["k", 12.01]]), json!(["playbackControl", ["seek", "t", -1]]),
                 json!(["queuePage", ["id",0,101]]), json!(["lyricsTimeline", ["id",7,0,1]])] {
        assert!(Call::parse(&args).is_none());
    }
}

#[test]
fn bern_cli_is_exposed_and_advertised_as_supported() {
    let mut plugins = Plugins::default();
    let script = "ui.toast([enji.supports('bern.cli'), typeof bern, typeof bern.cli].join('/'));";
    run(&mut plugins, declaration("sdk.cli", &[]), script);
    let calls = drain(&mut plugins);
    assert!(calls.contains(&("sdk.cli".into(), Call::Toast("true/object/function".into()))));
}

#[test]
fn desktop_lyrics_color_and_animation_frames_require_overlay_permission() {
    let mut plugins = Plugins::default();
    let script = "enji.on('frame', f => { desktopLyrics.setColor('#ff22aa', 0x11cc88); if (f.clear) desktopLyrics.clearColor(); });\n\
        ui.toast([enji.supports('desktopLyrics.color'), enji.supports('animation.frame'), desktopLyrics.setColor('bad')].join('/'));";
    run(&mut plugins, declaration("sdk.rgb", &["overlay"]), script);
    run(&mut plugins, declaration("sdk.denied", &[]), script);
    let init = drain(&mut plugins);
    assert!(init.contains(&("sdk.rgb".into(), Call::WantFrames(true))));
    assert!(init.contains(&("sdk.rgb".into(), Call::Toast("true/true/false".into()))));
    assert!(!init.contains(&("sdk.denied".into(), Call::WantFrames(true))));
    assert!(plugins.wants_frames());

    plugins.send_frame(&json!({ "time": 1.0, "dt": 0.025, "playing": true, "desktopLyrics": true, "clear": true }));
    let calls = drain(&mut plugins);
    assert_eq!(calls, vec![
        ("sdk.rgb".into(), Call::DesktopLyricColor { primary: Some(0xff22aa), secondary: Some(0x11cc88) }),
        ("sdk.rgb".into(), Call::DesktopLyricColor { primary: None, secondary: None }),
    ]);
    assert!(Call::parse(&json!(["desktopLyricColor", [0x1000000, null]])).is_none());
}
