use bramley_engine::{Command, PlayState, Queue, Repeat};
use serde_json::json;

use crate::wire::{self, CANT_PLAY, Failure, NOT_STARTED, NOTHING_PLAYABLE, Progress, QueueGate};

const FAILURE: &str =
    r#"{"error":{"type":"MusicTokenRequestError","message":"permission denied"}}"#;

#[test]
fn a_swift_snapshot_decodes_into_the_shared_snapshot_shape() {
    let snap = wire::snapshot(
        r#"{"state":"playing","position":12.5,"duration":201.0,"itemId":"1440857781","title":"Song","artist":"Band","queue":[{"id":"1440857781","title":"Song","duration":201.0},{"id":"i.abc"}],"queuePosition":0,"queueLength":9,"shuffle":true,"repeat":"all"}"#,
    )
    .unwrap();
    assert_eq!(snap.state, PlayState::Playing);
    assert_eq!(snap.position, Some(12.5));
    assert_eq!(snap.duration, Some(201.0));
    assert_eq!(snap.item_id.as_deref(), Some("1440857781"));
    assert_eq!(snap.title.as_deref(), Some("Song"));
    assert_eq!(snap.artist.as_deref(), Some("Band"));
    assert_eq!(snap.queue.len(), 2);
    assert_eq!(snap.queue[1].id, "i.abc");
    assert_eq!(snap.queue[1].duration, None);
    assert_eq!((snap.queue_position, snap.queue_length), (Some(0), 9));
    assert_eq!((snap.shuffle, snap.repeat), (true, Repeat::All));
}

#[test]
fn a_snapshot_without_modes_is_unshuffled_and_not_repeating() {
    let snap = wire::snapshot(r#"{"state":"paused","repeat":"sideways"}"#).unwrap();
    assert_eq!((snap.shuffle, snap.repeat), (false, Repeat::Off));
}

#[test]
fn an_idle_snapshot_with_no_current_entry_is_stopped_and_empty() {
    let snap = wire::snapshot(r#"{"state":"stopped","queue":[],"queueLength":0}"#).unwrap();
    assert_eq!(snap.state, PlayState::Stopped);
    assert_eq!(
        (snap.item_id, snap.position, snap.queue_position),
        (None, None, None)
    );
}

#[test]
fn each_swift_playback_state_maps_to_one_play_state() {
    for (word, state) in [
        ("playing", PlayState::Playing),
        ("paused", PlayState::Paused),
        ("loading", PlayState::Loading),
        ("stopped", PlayState::Stopped),
        ("seekingForward", PlayState::Stopped),
    ] {
        let snap = wire::snapshot(&format!(r#"{{"state":"{word}"}}"#)).unwrap();
        assert_eq!(snap.state, state, "{word}");
    }
}

#[test]
fn a_swift_failure_reply_becomes_a_typed_error_and_garbage_does_not_panic() {
    let failed = wire::snapshot(FAILURE).unwrap_err();
    assert_eq!(
        failed,
        Failure::new("MusicTokenRequestError", "permission denied")
    );
    assert_eq!(
        failed.to_string(),
        "MusicTokenRequestError: permission denied"
    );
    assert_eq!(
        wire::snapshot("not json")
            .unwrap_err()
            .to_string()
            .split(':')
            .next(),
        Some("MalformedReply")
    );
    assert!(wire::snapshot(r#"{"position":1.0}"#).is_err());
}

#[test]
fn tokens_and_authorization_decode_and_a_failure_never_echoes_a_jwt() {
    let pair = wire::tokens(r#"{"developer":"dev-value","user":"user-value"}"#).unwrap();
    assert_eq!(
        (pair.developer.as_str(), pair.user.as_str()),
        ("dev-value", "user-value")
    );
    assert_eq!(
        wire::authorization(r#"{"status":"notDetermined"}"#).unwrap(),
        "notDetermined"
    );
    let leaky = r#"{"error":{"type":"E","message":"bad eyJhbGciOi.eyJpc3MiOiJYIn0.c2lnbmF0dXJl"}}"#;
    assert_eq!(
        wire::tokens(leaky).err().unwrap().to_string(),
        "E: bad <jwt>"
    );
}

#[test]
fn commands_encode_to_the_swift_wire_and_unsupported_ones_are_dropped() {
    let songs = Queue::Songs(vec!["1".into(), "i.2".into()]);
    let wire_of = |c: Command| wire::command(&c);
    let listed = json!([{"library": true, "ids": ["i.2"]}, {"library": false, "ids": ["1"]}]);
    assert_eq!(
        wire_of(Command::SetQueue {
            queue: songs.clone(),
            start: 1
        }),
        Some(
            json!({"cmd": "setQueue", "kind": "songs", "ids": ["1", "i.2"], "batches": listed, "start": 1})
        )
    );
    assert_eq!(
        wire_of(Command::SetQueue {
            queue: Queue::Album("l.a".into()),
            start: 0
        }),
        Some(json!({"cmd": "setQueue", "kind": "album", "id": "l.a", "library": true, "start": 0}))
    );
    assert_eq!(
        wire_of(Command::PlayNext(Queue::Playlist("pl.u-1".into()))),
        Some(json!({"cmd": "playNext", "kind": "playlist", "id": "pl.u-1", "library": false}))
    );
    assert_eq!(
        wire_of(Command::Seek(30.5)),
        Some(json!({"cmd": "seek", "seconds": 30.5}))
    );
    assert_eq!(wire_of(Command::TopUp(songs)).unwrap()["cmd"], "append");
    assert_eq!(
        wire_of(Command::JumpTo {
            index: 3,
            id: "x".into()
        }),
        Some(json!({"cmd": "jump", "index": 3, "id": "x"}))
    );
    assert_eq!(
        wire_of(Command::Shuffle(true)),
        Some(json!({"cmd": "shuffle", "on": true}))
    );
    assert_eq!(
        wire_of(Command::Repeat(Repeat::One)),
        Some(json!({"cmd": "repeat", "mode": "one"}))
    );
    for plain in [
        ("play", Command::Play),
        ("pause", Command::Pause),
        ("next", Command::Next),
        ("previous", Command::Previous),
    ] {
        assert_eq!(wire_of(plain.1), Some(json!({"cmd": plain.0})));
    }
    assert_eq!(wire_of(Command::Volume(0.5)), None);
    assert_eq!(wire_of(Command::Shutdown), None);
}

#[test]
fn library_ids_are_told_apart_from_catalog_ids() {
    for id in ["i.abc", "l.abc", "p.abc"] {
        assert!(wire::is_library(id), "{id}");
    }
    for id in ["1440857781", "pl.u-abc", "a.1", "", "i"] {
        assert!(!wire::is_library(id), "{id}");
    }
}

#[test]
fn songs_are_resolved_in_batches_of_one_hundred_never_one_request_per_song() {
    let ids: Vec<String> = (0..250)
        .map(|n| format!("i.{n}"))
        .chain((0..101).map(|n| n.to_string()))
        .collect();
    let batches = wire::batches(&ids);
    let shape: Vec<(bool, usize)> = batches
        .iter()
        .map(|b| (b["library"] == true, b["ids"].as_array().unwrap().len()))
        .collect();
    assert_eq!(
        shape,
        [
            (true, 100),
            (true, 100),
            (true, 50),
            (false, 100),
            (false, 1)
        ]
    );
    assert_eq!(batches[0]["ids"][0], "i.0");
    assert_eq!(batches[3]["ids"][99], "99");
    assert!(wire::batches(&[]).is_empty());
}

#[test]
fn a_top_up_counts_as_background_work_and_every_other_command_as_applied() {
    let mut progress = Progress::default();
    let songs = Queue::Songs(vec!["1".into()]);
    for command in [
        Command::Play,
        Command::Volume(0.5),
        Command::TopUp(songs.clone()),
        Command::Seek(1.0),
        Command::TopUp(songs),
    ] {
        progress.record(&command);
    }
    assert_eq!((progress.applied, progress.background_done), (3, 2));
}

#[test]
fn the_play_after_a_failed_set_queue_is_not_sent_so_the_first_failure_is_the_one_shown() {
    let set = Command::SetQueue {
        queue: Queue::Songs(vec!["i.1".into()]),
        start: 0,
    };
    let mut gate = QueueGate::default();
    assert!(gate.permits(&set));
    gate.settle(&set, true);
    assert!(gate.permits(&Command::Shuffle(true)));
    assert!(!gate.permits(&Command::Play));
    assert!(gate.permits(&Command::Play), "only the Play that followed");
    gate.settle(&set, true);
    gate.settle(&set, false);
    assert!(gate.permits(&Command::Play), "a good SetQueue clears it");
    gate.settle(&Command::Seek(1.0), true);
    assert!(gate.permits(&Command::Play), "only SetQueue arms it");
}

fn play_failures() -> [(String, &'static str); 6] {
    let domain = "Error Domain=MPMusicPlayerControllerErrorDomain";
    let failure = |kind: &str, message: String| Failure::new(kind, &message).to_string();
    [
        (
            failure(
                "Command",
                "no playable items for the queue (resolved 0 of 3 asked)".into(),
            ),
            NOTHING_PLAYABLE,
        ),
        (
            failure("NSError", format!("{domain} Code=1 \"(null)\"")),
            NOT_STARTED,
        ),
        (
            failure("NSError", format!("{domain} Code=10 \"(null)\"")),
            CANT_PLAY,
        ),
        (
            failure("NSError", "Error Domain=NSURLErrorDomain Code=1".into()),
            CANT_PLAY,
        ),
        (
            failure(
                "Timeout",
                "MusicKit did not answer within 15 seconds".into(),
            ),
            CANT_PLAY,
        ),
        (
            failure("MusicDataRequest.Error", "status 404".into()),
            CANT_PLAY,
        ),
    ]
}

#[test]
fn each_known_play_failure_gets_its_own_text_and_every_other_one_the_generic_text() {
    for (raw, want) in play_failures() {
        assert_eq!(wire::friendly(&raw), want, "{raw}");
    }
}

#[test]
fn the_text_shown_for_a_play_failure_never_carries_its_raw_detail() {
    for (raw, _) in play_failures() {
        let shown = wire::friendly(&raw);
        for leak in ["Code", "Domain", "NSError", "Command:", "resolved", "404"] {
            assert!(!shown.contains(leak), "{shown} leaks {leak} from {raw}");
        }
        assert!(shown.contains("on Apple Music"), "{shown}");
    }
}
