#![allow(clippy::unwrap_used, clippy::panic)]

use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

#[test]
fn test_massive_dimensions_fail_safely() {
    // TransitionTable::new should reject dimensions that overflow or exceed limits.
    let res = TransitionTable::new(10_000_000_000, 10_000_000_000);
    assert!(
        res.is_err(),
        "TransitionTable::new should reject oversized tables without crashing"
    );
}

#[test]
fn test_transition_out_of_bounds() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);

    // Maliciously patch a target to be way out of bounds
    table.transitions_mut()[0] = 5000;

    let res = JitDfa::compile(&table);
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("exceeds state count"));
}

#[test]
fn test_accept_state_out_of_bounds() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.add_accept(5000, 0); // State doesn't exist

    let res = JitDfa::compile(&table);
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("exceeds state count"));
}

#[test]
fn test_accept_pattern_out_of_bounds() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // Add accept but don't specify pattern length for the pattern ID
    table.accept_states_mut().push((1, 9999));

    let res = JitDfa::compile(&table);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("has no length defined"));
}

#[test]
fn test_scan_zero_length_input() {
    let table = TransitionTable::new(2, 256).unwrap();
    let jit = JitDfa::compile(&table).unwrap();

    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(&[], &mut matches);
    assert_eq!(count, 0);
}

#[test]
fn test_scan_gigantic_buffer_size() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    // Use a large zero buffer (cheap memory wise compared to alloc)
    // We expect 0 matches since our pattern is 'a'
    let huge_input = vec![0u8; 10_000_000];
    let mut matches = vec![Match::from_parts(0, 0, 0); 1];

    let count = jit.scan(&huge_input, &mut matches);
    assert_eq!(count, 0);
}

#[test]
fn test_scan_near_usize_max_matches_buffer() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();

    // With the new semantics, if we pass an empty buffer, scan returns 0
    // because it caps at buffer size. scan_count returns the true count.
    let count = jit.scan_count(b"a");
    assert_eq!(count, 1);

    // Testing scan with an empty buffer should return 0 safely without crashing.
    let scanned = jit.scan(b"a", &mut []);
    assert_eq!(scanned, 0);
}

#[test]
fn test_multiple_accepts_same_state() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.add_accept(1, 0);
    table.add_accept(1, 1);
    table.set_pattern_length(0, 1);
    table.set_pattern_length(1, 1);

    let res = JitDfa::compile(&table);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("multiple accept patterns"));
}
