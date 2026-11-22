//! Ground truth for transcripts: an opt-in setting records the raw bytes in and
//! out of a session to a local file for bug reports, and the plugin test harness replays a
//! recorded exchange. Nothing is uploaded. Hosts a plugin reaches over the internet are recorded
//! too, so a bug report shows what left the machine.

use super::*;

#[test]
fn bytes_are_recorded_in_order_with_their_direction_and_time() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, b"\x01\x02");
    t.record(Direction::In, 15, b"OK\n");
    let e = t.entries();
    assert_eq!(e.len(), 2);
    assert_eq!(
        (e[0].direction, e[0].at_ms, e[0].bytes.as_slice()),
        (Direction::Out, 0, &b"\x01\x02"[..])
    );
    assert_eq!((e[1].direction, e[1].at_ms), (Direction::In, 15));
}

#[test]
fn a_transcript_round_trips_through_json() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 5, &[0, 255, 16]);
    t.record(Direction::In, 9, b"ok");
    t.record_host(12, "cloud.example.com");
    let json = t.to_json().unwrap();
    assert_eq!(Transcript::from_json(&json).unwrap(), t);
}

#[test]
fn the_json_stores_bytes_as_hex_so_it_is_readable() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, &[0xDE, 0xAD, 0xBE, 0xEF]);
    let v: serde_json::Value = serde_json::from_str(&t.to_json().unwrap()).unwrap();
    assert_eq!(v["entries"][0]["hex"], "deadbeef");
    assert_eq!(v["entries"][0]["dir"], "out");
}

#[test]
fn only_one_direction_can_be_listed() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, b"a");
    t.record(Direction::In, 1, b"b");
    t.record(Direction::Out, 2, b"c");
    let out: Vec<&[u8]> = t
        .in_direction(Direction::Out)
        .map(|e| e.bytes.as_slice())
        .collect();
    assert_eq!(out, vec![&b"a"[..], b"c"]);
}

#[test]
fn contacted_hosts_are_listed_once_each() {
    let mut t = Transcript::default();
    t.record_host(1, "a.example.com");
    t.record_host(2, "b.example.com");
    t.record_host(3, "a.example.com");
    assert_eq!(
        t.hosts(),
        vec!["a.example.com".to_owned(), "b.example.com".to_owned()]
    );
}

#[test]
fn damaged_transcripts_are_errors() {
    assert!(Transcript::from_json("not json").is_err());
    assert!(
        Transcript::from_json(r#"{"entries":[{"t_ms":0,"dir":"sideways","hex":"00"}]}"#).is_err()
    );
    assert!(Transcript::from_json(r#"{"entries":[{"t_ms":0,"dir":"in","hex":"zz"}]}"#).is_err());
    assert!(
        Transcript::from_json(r#"{"entries":[{"t_ms":0,"dir":"in","hex":"abc"}]}"#).is_err(),
        "odd length"
    );
}

#[test]
fn a_transcript_is_written_to_a_file_for_the_bug_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut t = Transcript::default();
    t.record(Direction::In, 0, b"x");
    t.save(&path).unwrap();
    assert_eq!(Transcript::load(&path).unwrap(), t);
}

#[test]
fn an_empty_transcript_is_valid() {
    let t = Transcript::default();
    assert_eq!(Transcript::from_json(&t.to_json().unwrap()).unwrap(), t);
    assert_eq!(t.entries().len(), 0);
}
