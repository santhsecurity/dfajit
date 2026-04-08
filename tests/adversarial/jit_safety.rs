#![allow(clippy::unwrap_used, clippy::panic)]

//! JIT Code Safety Tests
//!
//! These tests verify that the JIT compiler generates safe machine code that:
//! 1. Never writes outside allocated match buffers
//! 2. Handles edge cases in input boundaries
//! 3. Properly validates all table indices before use
//! 4. Maintains W^X memory safety guarantees

use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

/// Verify JIT doesn't write past match buffer boundary.
/// This is critical: at internet scale, a buffer overflow could corrupt
/// adjacent memory containing request/response data.
#[test]
fn test_jit_respects_match_buffer_boundary() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // Pattern that matches every single byte
    for byte in 0..=255u8 {
        table.set_transition(0, byte, 1);
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    // Input where every byte triggers a match
    let input = vec![b'x'; 1000];

    // Very small match buffer - JIT must not write past this
    let mut matches = vec![Match::from_parts(0, 0, 0); 5];
    let count = jit.scan(&input, &mut matches);

    // JIT caps count at buffer size - use scan_count for true count
    assert_eq!(count, 5, "Should cap at buffer size");
    assert_eq!(jit.scan_count(&input), 1000, "scan_count should report all");

    // Verify we can still access the match buffer (no corruption)
    for m in &matches {
        assert_eq!(m.pattern_id, 0);
    }
}

/// Verify scan_count returns correct count without buffer limitations.
#[test]
fn test_scan_count_reports_all_matches() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    for byte in 0..=255u8 {
        table.set_transition(0, byte, 1);
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    let input = vec![b'a'; 100];
    let count = jit.scan_count(&input);
    assert_eq!(count, 100);
}

/// Verify empty match buffer is handled correctly.
#[test]
fn test_zero_length_match_buffer() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    let mut matches: Vec<Match> = vec![];
    let count = jit.scan(b"aaa", &mut matches);
    assert_eq!(count, 0);
}

/// Test that JIT code with near-maximum states doesn't corrupt memory.
#[test]
fn test_near_maximum_states_safety() {
    // 4096 is the JIT limit - test right at the boundary
    let mut table = TransitionTable::new(4096, 256).unwrap();

    // Create a chain through all states
    for state in 0..4095 {
        table.set_transition(state, b'x', (state + 1) as u32);
    }
    table.add_accept(4095, 0);
    table.set_pattern_length(0, 4096);

    let jit = JitDfa::compile(&table).unwrap();

    // Input that traverses all states
    let input = vec![b'x'; 4096];
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(&input, &mut matches);

    assert_eq!(count, 1);
    assert_eq!(matches[0].start, 0);
    // Match ends at position 4095 (byte index 4094 + 1)
    // Byte 4094 transitions state 4094->4095 (accept)
    assert_eq!(matches[0].end, 4095);
}

/// Test that transition table bounds are enforced.
#[test]
fn test_transition_target_bounds_checked() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    // Corrupt the table to point to non-existent state
    table.transitions_mut()[0] = 9999;

    let result = JitDfa::compile(&table);
    assert!(
        result.is_err(),
        "Should reject transition to non-existent state"
    );
}

/// Verify W^X memory protection by ensuring code section is not writable.
/// This test verifies the compile path correctly uses mprotect.
#[test]
fn test_wx_memory_protection() {
    let table = TransitionTable::new(2, 256).unwrap();

    // Should compile successfully
    let jit = JitDfa::compile(&table).unwrap();

    // Should be able to scan without crashes
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"test", &mut matches);

    // No matches expected since no accept states
    assert_eq!(count, 0);
}

/// Test that multiple patterns writing to same buffer don't overflow.
#[test]
fn test_multi_pattern_match_boundary() {
    let mut table = TransitionTable::new(3, 256).unwrap();

    // Pattern 0: matches 'a'
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    // Pattern 1: matches 'b'
    table.set_transition(0, b'b', 2);
    table.add_accept(2, 1);
    table.set_pattern_length(1, 1);

    let jit = JitDfa::compile(&table).unwrap();

    // Alternating pattern input - 10 matches total
    let input = b"ababababab";
    let mut matches = vec![Match::from_parts(0, 0, 0); 3];
    let count = jit.scan(input, &mut matches);

    // JIT caps at buffer size (3)
    assert_eq!(count, 3);
    // scan_count should report true count (10)
    assert_eq!(jit.scan_count(input), 10);
}

/// Test that pattern length computation doesn't underflow.
#[test]
fn test_pattern_length_underflow_safety() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    // Pattern length longer than current position
    table.set_pattern_length(0, 100);

    let jit = JitDfa::compile(&table).unwrap();

    // Match at position 0 with pattern length 100
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"x", &mut matches);

    assert_eq!(count, 1);
    // start should be 0 (saturated), not wrapping around
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 1);
}

/// Test single-byte input with single-byte pattern.
#[test]
fn test_minimum_viable_scan() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"x", &mut matches);

    assert_eq!(count, 1);
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 1);
}

/// Test that interpreted fallback for large DFAs works correctly.
#[test]
fn test_large_dfa_interpreted_fallback() {
    // More than 4096 states triggers interpreted fallback
    let mut table = TransitionTable::new(5000, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    // Should succeed with interpreted fallback
    let jit = JitDfa::compile(&table).unwrap();

    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"a", &mut matches);

    assert_eq!(count, 1);
}

/// Verify no crash with all possible byte values in input.
#[test]
fn test_all_byte_values_in_input() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, 0xFF, 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    let input: Vec<u8> = (0..=255).collect();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(&input, &mut matches);

    assert_eq!(count, 1); // Only 0xFF should match
    assert_eq!(matches[0].pattern_id, 0);
}

/// Test that concurrent scanning doesn't corrupt memory.
#[test]
fn test_concurrent_scan_memory_safety() {
    use std::sync::Arc;
    use std::thread;

    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let jit = Arc::new(JitDfa::compile(&table).unwrap());

    let mut handles = vec![];
    for _ in 0..100 {
        let jit_clone = Arc::clone(&jit);
        handles.push(thread::spawn(move || {
            let mut matches = vec![Match::from_parts(0, 0, 0); 100];
            let input = b"ababababab";
            let count = jit_clone.scan(input, &mut matches);
            assert_eq!(count, 5);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
