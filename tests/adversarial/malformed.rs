#![allow(clippy::unwrap_used, clippy::panic)]

use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

#[test]
fn test_from_bytes_truncated_data() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let bytes = table.to_bytes();

    // Test truncating at various boundaries
    for len in 0..bytes.len() {
        let res = TransitionTable::from_bytes(&bytes[..len]);
        assert!(
            res.is_err(),
            "Truncated buffer length {} should fail gracefully",
            len
        );
    }
}

#[test]
fn test_from_bytes_invalid_state_count() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    let mut bytes = table.to_bytes();

    // Patch state_count to be huge, causing slice bounds to check
    let huge: u32 = 1_000_000;
    bytes[0..4].copy_from_slice(&huge.to_le_bytes());

    let res = TransitionTable::from_bytes(&bytes);
    assert!(
        res.is_err(),
        "Forged state_count should safely fail slice boundary checks without crashing"
    );
}

#[test]
fn test_scan_invalid_utf8_sequence() {
    let jit = JitDfa::from_patterns(&[b"invalid\xFF"]).unwrap();

    // We try to match with invalid utf8 inside and outside the expected pattern.
    // Length breakdown:
    // "hello " = 6 bytes
    // "\x80 \xFF \xC3\x28 " = 7 bytes
    // "invalid\xFF" = 8 bytes
    // " \x00" = 2 bytes
    // Total prefix before match = 13 bytes.
    let malicious_input = b"hello \x80 \xFF \xC3\x28 invalid\xFF \x00";
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];

    let count = jit.scan(malicious_input, &mut matches);
    assert_eq!(
        count, 1,
        "Should correctly match byte patterns within invalid UTF-8 without crashing"
    );
    assert_eq!(matches[0].start, 13);
    assert_eq!(matches[0].end, 21);
}

#[test]
fn test_scan_combining_characters() {
    let jit = JitDfa::from_patterns(&["é".as_bytes()]).unwrap();

    // Input with zero-width joiners and combining accents
    let malformed_input = "éx a\u{0301}b e\u{0301}".as_bytes();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];

    let count = jit.scan(malformed_input, &mut matches);
    assert_eq!(
        count, 2,
        "Should safely process and match complex unicode bytes"
    );
}
