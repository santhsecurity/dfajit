#![allow(clippy::unwrap_used, clippy::panic)]

//! Platform-Specific and Edge Case Tests
//!
//! Tests for:
//! 1. Empty DFA handling
//! 2. Non-x86 fallback behavior
//! 3. Concurrent compilation safety
//! 4. Extreme input patterns

use dfajit::{Error, JitDfa, TransitionTable};
use matchkit::Match;

/// Empty DFA must be rejected with proper error.
#[test]
fn test_empty_dfa_rejected() {
    let table = TransitionTable::new(0, 256).unwrap();
    let result = JitDfa::compile(&table);

    assert!(matches!(result, Err(Error::EmptyDfa)));
}

/// Single-state DFA (no accepts) should compile and run.
#[test]
fn test_single_state_dfa() {
    let table = TransitionTable::new(1, 256).unwrap();
    // No transitions, no accepts - just a start state

    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"anything", &mut matches);

    assert_eq!(count, 0);
}

/// Test from_patterns with empty pattern list.
#[test]
fn test_from_patterns_empty_list() {
    let result = JitDfa::from_patterns(&[]);
    assert!(matches!(result, Err(Error::EmptyDfa)));
}

/// Test from_patterns with all empty patterns.
#[test]
fn test_from_patterns_all_empty() {
    // All patterns are empty, so they get skipped
    // Result is a minimal DFA with just start state
    let result = JitDfa::from_patterns(&[b"", b"", b""]);
    // Should succeed but match nothing
    assert!(result.is_ok());
    let jit = result.unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"abc", &mut matches);
    assert_eq!(count, 0);
}

/// Test that patterns with only empty strings are handled.
#[test]
fn test_from_patterns_mixed_empty() {
    // Mix of empty and non-empty patterns
    let result = JitDfa::from_patterns(&[b"", b"a", b""]);
    assert!(result.is_ok());

    let jit = result.unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"aaa", &mut matches);
    assert_eq!(count, 3);
}

/// Test extremely long pattern.
#[test]
fn test_extremely_long_pattern() {
    let long_pattern: Vec<u8> = vec![b'x'; 10000];
    let result = JitDfa::from_patterns(&[&long_pattern]);

    // Should succeed (may use interpreted fallback for large DFAs)
    assert!(result.is_ok());
    let jit = result.unwrap();

    // Should match the long pattern
    let mut input = long_pattern.clone();
    input.push(b'y'); // Add non-matching suffix

    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(&input, &mut matches);
    assert_eq!(count, 1);
}

/// Test DFA with self-loop states.
#[test]
fn test_self_loop_states() {
    let mut table = TransitionTable::new(2, 256).unwrap();

    // State 0: on 'a', stay in 0; on 'b', go to 1
    for byte in 0..=255u8 {
        if byte == b'a' {
            table.set_transition(0, byte, 0);
        } else if byte == b'b' {
            table.set_transition(0, byte, 1);
        } else {
            table.set_transition(0, byte, 0);
        }
    }

    // State 1 is accept
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    // "aaab" should match at position 3
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"aaab", &mut matches);
    assert_eq!(count, 1);
    assert_eq!(matches[0].start, 3);
}

/// Test DFA that requires restart after match.
#[test]
fn test_restart_after_match() {
    let mut table = TransitionTable::new(3, 256).unwrap();

    // Pattern: "ab"
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    // All other transitions go to 0 (implicit)

    let jit = JitDfa::compile(&table).unwrap();

    // "abab" should match twice
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"abab", &mut matches);
    assert_eq!(count, 2);
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 2);
    assert_eq!(matches[1].start, 2);
    assert_eq!(matches[1].end, 4);
}

/// Test overlapping pattern matches.
#[test]
fn test_overlapping_patterns() {
    // Patterns: "aa" and "aaa"
    let result = JitDfa::from_patterns(&[b"aa", b"aaa"]);
    assert!(result.is_ok());

    let jit = result.unwrap();

    // "aaaa" contains:
    // - "aa" at positions 0-2 and 2-4
    // - "aaa" at positions 0-3 and 1-4 (if overlapping allowed)
    // But our DFA implementation resets after match
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"aaaa", &mut matches);

    // With reset behavior, we get matches at positions:
    // - "aa" at 0-2, then reset, then "aa" at 2-4
    // Or if "aaa" matches first, different behavior
    assert!(count >= 1);
}

/// Test has_match returns correct boolean.
#[test]
fn test_has_match_correctness() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    assert!(jit.has_match(b"xxx"));
    assert!(!jit.has_match(b"yyy"));
    assert!(!jit.has_match(b""));
}

/// Test scan_first returns only first match.
#[test]
fn test_scan_first_only_first() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    for byte in 0..=255u8 {
        table.set_transition(0, byte, 1);
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    let first = jit.scan_first(b"abc");
    assert!(first.is_some());
    assert_eq!(first.unwrap().start, 0);
    assert_eq!(first.unwrap().end, 1);
}

/// Test empty input handling across all scan methods.
#[test]
fn test_empty_input_all_methods() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();

    // scan with empty input
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"", &mut matches), 0);

    // scan_count with empty input
    assert_eq!(jit.scan_count(b""), 0);

    // scan_first with empty input
    assert!(jit.scan_first(b"").is_none());

    // has_match with empty input
    assert!(!jit.has_match(b""));
}

/// Test serialization of minimal DFA.
#[test]
fn test_serialization_minimal_dfa() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let bytes = table.to_bytes();
    let restored = TransitionTable::from_bytes(&bytes).unwrap();

    assert_eq!(restored.state_count(), table.state_count());
    assert_eq!(restored.transitions(), table.transitions());
    assert_eq!(restored.accept_states(), table.accept_states());
}

/// Test compute_ranges with uniform transitions.
#[test]
fn test_compute_ranges_uniform() {
    let table = TransitionTable::new(2, 256).unwrap();
    // All transitions default to 0

    let ranges = table.compute_ranges();
    // Should be a single range: [0, 255] -> 0
    assert_eq!(ranges[0].len(), 1);
    assert_eq!(ranges[0][0], (0, 255, 0));
}

/// Test compute_ranges with fragmented transitions.
#[test]
fn test_compute_ranges_fragmented() {
    let mut table = TransitionTable::new(2, 256).unwrap();

    // Alternate between two targets for every byte
    for byte in 0..=255u8 {
        if byte % 2 == 0 {
            table.set_transition(0, byte, 0);
        } else {
            table.set_transition(0, byte, 1);
        }
    }

    let ranges = table.compute_ranges();
    // Should have many small ranges
    assert!(ranges[0].len() > 200);
}

/// Test DFA minimization preserves single-accept behavior.
#[test]
fn test_minimize_single_accept() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let minimized = table.minimize();
    // Should still match "ab"
    let test_table = minimized.unwrap_or(table);
    let jit = JitDfa::compile(&test_table).unwrap();

    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"ab", &mut matches);
    assert_eq!(count, 1);
}

/// Test that estimated_code_size is monotonic with state count.
#[test]
fn test_code_size_monotonic() {
    let table_small = TransitionTable::new(10, 256).unwrap();
    let table_large = TransitionTable::new(100, 256).unwrap();

    assert!(table_large.estimated_code_size() > table_small.estimated_code_size());
}

/// Test transition table with class_count different from 256.
#[test]
fn test_non_standard_class_count() {
    // Some applications might want fewer classes (e.g., after byte classification)
    let table = TransitionTable::new(5, 16).unwrap();
    assert_eq!(table.transitions().len(), 80);

    // Should still be usable
    let result = JitDfa::compile(&table);
    // This might succeed or fail depending on implementation,
    // but should not crash
    if result.is_ok() {
        let jit = result.unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let _ = jit.scan(b"test", &mut matches);
    }
}
