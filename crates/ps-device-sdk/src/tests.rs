//! Ground truth for the plugin SDK helpers: line buffering, checksums, and
//! acknowledgement-based flow control (GRBL-style character counting). Checksum values are the
//! standard definitions (Marlin's XOR line checksum, an 8-bit sum).

use super::*;

// ---- line buffering ----

#[test]
fn complete_lines_come_out_and_a_partial_line_waits() {
    let mut b = LineBuffer::default();
    assert_eq!(b.push(b"ok\nerr"), vec!["ok".to_owned()]);
    assert_eq!(b.push(b"or:9\r\n"), vec!["error:9".to_owned()]);
    assert_eq!(b.pending(), "");
}

#[test]
fn several_lines_in_one_chunk_are_all_returned_in_order() {
    let mut b = LineBuffer::default();
    assert_eq!(
        b.push(b"a\nb\r\nc\n"),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
    );
}

#[test]
fn bytes_arriving_one_at_a_time_still_make_lines() {
    let mut b = LineBuffer::default();
    let mut got = Vec::new();
    for byte in b"<Idle|MPos:0.000,0.000,0.000>\nok\n" {
        got.extend(b.push(&[*byte]));
    }
    assert_eq!(
        got,
        vec!["<Idle|MPos:0.000,0.000,0.000>".to_owned(), "ok".to_owned()]
    );
}

#[test]
fn empty_lines_are_dropped() {
    let mut b = LineBuffer::default();
    assert_eq!(b.push(b"\n\r\nok\n\n"), vec!["ok".to_owned()]);
}

#[test]
fn a_runaway_line_is_cut_off_rather_than_growing_forever() {
    let mut b = LineBuffer::with_limit(16);
    let lines = b.push(&[b'x'; 100]);
    assert_eq!(lines.len(), 0);
    assert!(b.pending().len() <= 16, "{}", b.pending().len());
    // After the overflow the next newline still ends a line.
    assert_eq!(b.push(b"\n").len(), 1);
}

#[test]
fn non_utf8_bytes_do_not_panic() {
    let mut b = LineBuffer::default();
    let lines = b.push(&[0xFF, 0xFE, b'\n']);
    assert_eq!(lines.len(), 1);
}

// ---- checksums ----

#[test]
fn the_xor_checksum_is_the_xor_of_every_byte() {
    assert_eq!(xor_checksum(b""), 0);
    assert_eq!(xor_checksum(b"A"), 0x41);
    assert_eq!(xor_checksum(b"AB"), 0x41 ^ 0x42);
    // Worked by hand: 4E 30 20 4D 31 31 30 20 4E 30 XOR to 0x7D = 125, the classic Marlin example
    // "N0 M110 N0*125".
    assert_eq!(xor_checksum(b"N0 M110 N0"), 125);
}

#[test]
fn the_eight_bit_sum_wraps() {
    assert_eq!(sum_checksum(&[200, 100]), 44);
    assert_eq!(sum_checksum(b""), 0);
}

#[test]
fn a_numbered_line_with_a_checksum_follows_the_marlin_format() {
    assert_eq!(numbered_line(0, "M110 N0"), "N0 M110 N0*125");
}

// ---- acknowledgement-based flow control ----

#[test]
fn the_receive_buffer_limits_how_much_can_be_in_flight() {
    let mut f = AckFlow::new(20);
    assert!(f.can_send(10)); // 10 chars + newline
    f.sent(10);
    assert!(f.can_send(8), "11 + 9 = 20 fits exactly");
    f.sent(8);
    assert!(!f.can_send(1), "the buffer is full");
    assert_eq!(f.in_flight(), 20);
}

#[test]
fn an_acknowledgement_frees_the_oldest_line() {
    let mut f = AckFlow::new(30);
    f.sent(9); // 10 with newline
    f.sent(14); // 15 with newline
    assert_eq!(f.in_flight(), 25);
    assert_eq!(f.acked(), Some(10));
    assert_eq!(f.in_flight(), 15);
    assert!(f.can_send(14));
    assert_eq!(f.acked(), Some(15));
    assert_eq!(f.acked(), None, "nothing left to acknowledge");
    assert_eq!(f.in_flight(), 0);
}

#[test]
fn a_line_longer_than_the_buffer_can_never_be_sent() {
    let f = AckFlow::new(10);
    assert!(!f.can_send(10), "10 chars plus newline is 11");
    assert!(f.can_send(9));
}

#[test]
fn resetting_forgets_everything_in_flight() {
    let mut f = AckFlow::new(127);
    f.sent(50);
    f.reset();
    assert_eq!(f.in_flight(), 0);
    assert_eq!(f.acked(), None);
}

#[test]
fn many_small_lines_fill_the_buffer_by_characters_not_by_count() {
    let mut f = AckFlow::new(127);
    let mut count = 0;
    while f.can_send(3) {
        f.sent(3);
        count += 1;
    }
    assert_eq!(count, 31, "127 / 4 characters per line");
}
