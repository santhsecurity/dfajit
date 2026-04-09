#![allow(clippy::missing_panics_doc)]
//! JIT compilation of DFA transition tables to native x86_64 code.
//!
//! `dfajit` converts a DFA state machine (as produced by `warpstate`) into
//! a native function that scans input bytes without table lookup indirection.
//! Each DFA state becomes a labeled basic block with a 256-entry jump table
//! indexed by the input byte.
//!
//! # Architecture
//!
//! The compiled function has this signature:
//!
//! ```text
//! fn(input: *const u8, len: usize, matches: *mut Match, max_matches: usize) -> usize
//! ```
//!
//! Returns the number of matches written. The function:
//! 1. Loads the start state
//! 2. For each input byte: loads byte, indexes into jump table, jumps to next state
//! 3. At accept states: writes match to output buffer, increments count
//! 4. Returns match count
//!
//! # Example
//!
//! ```rust
//! use dfajit::{TransitionTable, JitDfa};
//!
//! // Build a simple 3-state DFA that matches "ab"
//! let mut table = TransitionTable::new(3, 256).unwrap();
//! table.set_transition(0, b'a', 1);  // state 0 --'a'--> state 1
//! table.set_transition(1, b'b', 2);  // state 1 --'b'--> state 2
//! table.add_accept(2, 0);            // state 2 accepts pattern 0
//! // All other transitions go to state 0 (dead/restart)
//!
//! let jit = JitDfa::compile(&table).unwrap();
//! // Pass sufficiently large array slice since JIT doesn't re-allocate
//! let mut matches = vec![matchkit::Match::from_parts(0, 0, 0); 10];
//! let count = jit.scan(b"xabxab", &mut matches);
//! assert_eq!(count, 2);
//! ```

#![warn(missing_docs, clippy::pedantic)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::todo,
        clippy::unimplemented,
        clippy::panic
    )
)]
#![allow(
    clippy::assigning_clones,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_range_loop,
    clippy::ptr_as_ptr,
    clippy::similar_names,
    clippy::too_many_lines
)]

mod codegen;
mod dfa;
mod error;
mod table;

pub use dfa::JitDfa;
pub use error::{Error, Result};
pub use table::TransitionTable;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use matchkit::Match;

    fn new_table(state_count: usize, class_count: usize) -> TransitionTable {
        TransitionTable::new(state_count, class_count).unwrap()
    }

    fn simple_ab_table() -> TransitionTable {
        // 3-state DFA: matches "ab"
        // State 0: start. 'a' -> 1, else -> 0
        // State 1: saw 'a'. 'b' -> 2, 'a' -> 1, else -> 0
        // State 2: accept. Restart: 'a' -> 1, else -> 0
        let mut table = new_table(3, 256);
        for state in 0..3 {
            for byte in 0..=255u8 {
                table.set_transition(state, byte, 0); // default: back to start
            }
            table.set_transition(state, b'a', 1); // 'a' always goes to state 1
        }
        table.set_transition(1, b'b', 2);
        table.add_accept(2, 0);
        table.set_pattern_length(0, 2);
        table
    }

    #[test]
    fn compile_simple_dfa() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();
        assert_eq!(jit.state_count(), 3);
        assert_eq!(jit.pattern_count(), 1);
    }

    #[test]
    fn scan_finds_matches() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"xabxab", &mut matches);
        assert_eq!(count, 2);
        assert_eq!(matches[0].start, 1);
        assert_eq!(matches[0].end, 3);
        assert_eq!(matches[1].start, 4);
        assert_eq!(matches[1].end, 6);
    }

    #[test]
    fn scan_empty_input() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"", &mut matches);
        assert_eq!(count, 0);
    }

    #[test]
    fn scan_count_matches_scan_len() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];

        assert_eq!(jit.scan_count(b"xabxab"), jit.scan(b"xabxab", &mut matches));
    }

    #[test]
    fn scan_no_match() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"xxxxxx", &mut matches);
        assert_eq!(count, 0);
    }

    #[test]
    fn scan_consecutive_matches() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"ababab", &mut matches);
        assert_eq!(count, 3);
    }

    #[test]
    fn scan_first_returns_first_match_only() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();

        let first = jit.scan_first(b"zzabzzab").unwrap();
        assert_eq!(first.start, 2);
        assert_eq!(first.end, 4);
    }

    #[test]
    fn has_match_reports_presence() {
        let table = simple_ab_table();
        let jit = JitDfa::compile(&table).unwrap();

        assert!(jit.has_match(b"xab"));
        assert!(!jit.has_match(b"zzz"));
    }

    #[test]
    fn empty_dfa_rejected() {
        let table = new_table(0, 256);
        assert!(JitDfa::compile(&table).is_err());
    }

    #[test]
    fn invalid_table_size_rejected() {
        let mut table = new_table(3, 256);
        table.transitions_mut().truncate(10); // corrupt
        assert!(JitDfa::compile(&table).is_err());
    }

    #[test]
    fn multi_pattern_dfa() {
        // 4-state DFA: matches "a" (pattern 0) and "b" (pattern 1)
        let mut table = new_table(3, 256);
        for byte in 0..=255u8 {
            table.set_transition(0, byte, 0);
            table.set_transition(1, byte, 0);
            table.set_transition(2, byte, 0);
        }
        table.set_transition(0, b'a', 1);
        table.set_transition(0, b'b', 2);
        table.set_transition(1, b'a', 1);
        table.set_transition(1, b'b', 2);
        table.set_transition(2, b'a', 1);
        table.set_transition(2, b'b', 2);
        table.add_accept(1, 0);
        table.add_accept(2, 1);
        table.set_pattern_length(0, 1);
        table.set_pattern_length(1, 1);

        let jit = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"ab", &mut matches);
        assert_eq!(count, 2);
        assert_eq!(matches[0].pattern_id, 0); // 'a'
        assert_eq!(matches[1].pattern_id, 1); // 'b'
    }

    #[test]
    fn from_patterns_builds_literal_matcher() {
        let jit = JitDfa::from_patterns(&[b"foo", b"bar"]).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];

        let count = jit.scan(b"foo bar", &mut matches);
        assert_eq!(count, 2);
        assert_eq!(matches[0].end, 3);
        assert_eq!(matches[1].end, 7);
    }

    #[test]
    fn from_patterns_rejects_empty_pattern_set() {
        assert!(JitDfa::from_patterns(&[]).is_err());
    }

    #[test]
    fn from_patterns_ignores_empty_literals() {
        let jit = JitDfa::from_patterns(&[b"", b"x"]).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];

        assert_eq!(jit.scan(b"x", &mut matches), 1);
        assert_eq!(matches[0].pattern_id, 1);
    }

    #[test]
    fn minimize_reduces_redundant_states() {
        // Build a DFA with redundant states:
        // States 0, 1, 2 all transition to state 3 on 'x'
        // States 1, 2 are equivalent (same transitions, neither accept)
        let mut table = new_table(4, 256);
        for s in 0..4 {
            for b in 0..=255u8 {
                table.set_transition(s, b, 0);
            }
        }
        table.set_transition(0, b'x', 3);
        table.set_transition(1, b'x', 3);
        table.set_transition(2, b'x', 3);
        table.add_accept(3, 0);
        table.set_pattern_length(0, 1);

        if let Some(minimized) = table.minimize() {
            assert!(minimized.state_count() < table.state_count());
            // The minimized DFA should still match 'x'
            let jit = JitDfa::compile(&minimized).unwrap();
            let mut matches = vec![Match::from_parts(0, 0, 0); 10];
            let count = jit.scan(b"x", &mut matches);
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn minimize_already_minimal() {
        let table = simple_ab_table();
        // The simple 3-state DFA should already be near-minimal
        // (state 0 = start, state 1 = saw 'a', state 2 = accept)
        // minimize() returns None if no reduction possible
        let result = table.minimize();
        // Either None (already minimal) or a smaller table
        if let Some(minimized) = result {
            assert!(minimized.state_count() <= table.state_count());
        }
    }

    #[test]
    fn serialization_round_trip() {
        let table = simple_ab_table();
        let bytes = table.to_bytes();
        let restored = TransitionTable::from_bytes(&bytes).unwrap();
        assert_eq!(restored.state_count(), table.state_count());
        assert_eq!(restored.class_count(), table.class_count());
        assert_eq!(restored.transitions(), table.transitions());
        assert_eq!(restored.accept_states(), table.accept_states());
        assert_eq!(restored.pattern_lengths(), table.pattern_lengths());

        // Verify the restored table produces the same JIT results
        let jit = JitDfa::compile(&restored).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"xabxab", &mut matches);
        assert_eq!(count, 2);
    }

    #[test]
    fn serialization_rejects_truncated() {
        let table = simple_ab_table();
        let bytes = table.to_bytes();
        let truncated = &bytes[..10];
        assert!(TransitionTable::from_bytes(truncated).is_err());
    }

    #[test]
    fn serialization_rejects_truncated_accept_metadata() {
        let table = simple_ab_table();
        let mut bytes = table.to_bytes();
        bytes.truncate(bytes.len() - 6);
        assert!(TransitionTable::from_bytes(&bytes).is_err());
    }

    #[test]
    fn serialization_rejects_truncated_pattern_lengths() {
        let table = simple_ab_table();
        let mut bytes = table.to_bytes();
        bytes.truncate(bytes.len() - 2);
        assert!(TransitionTable::from_bytes(&bytes).is_err());
    }

    #[test]
    fn is_jit_eligible_small() {
        let table = simple_ab_table();
        assert!(table.is_jit_eligible());
    }

    #[test]
    fn estimated_code_size() {
        let table = simple_ab_table();
        let size = table.estimated_code_size();
        assert!(size > 0);
        assert!(size < 100_000); // 3-state DFA should be small
    }

    #[test]
    fn large_dfa_is_not_jit_eligible() {
        let table = new_table(4097, 256);
        assert!(!table.is_jit_eligible());
    }

    #[test]
    fn minimize_preserves_behavior() {
        let mut table = new_table(5, 256);
        for s in 0..5 {
            for b in 0..=255u8 {
                table.set_transition(s, b, 0);
            }
            table.set_transition(s, b'a', 1);
        }
        table.set_transition(1, b'b', 2);
        table.set_transition(2, b'c', 3);
        table.add_accept(3, 0);
        table.set_pattern_length(0, 3);

        let original = JitDfa::compile(&table).unwrap();
        let input = b"xabcxabc";
        let mut orig_matches = vec![Match::from_parts(0, 0, 0); 10];
        let orig_count = original.scan(input, &mut orig_matches);

        if let Some(minimized) = table.minimize() {
            let min_jit = JitDfa::compile(&minimized).unwrap();
            let mut min_matches = vec![Match::from_parts(0, 0, 0); 10];
            let min_count = min_jit.scan(input, &mut min_matches);
            assert_eq!(orig_count, min_count);
        }
    }

    #[test]
    fn compute_ranges_collapses_consecutive_targets() {
        let mut table = new_table(2, 256);
        for byte in 0..=u8::MAX {
            table.set_transition(0, byte, 0);
        }
        for byte in b'a'..=b'z' {
            table.set_transition(0, byte, 1);
        }

        let ranges = table.compute_ranges();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], vec![(0, 96, 0), (97, 122, 1), (123, 255, 0)]);
    }

    #[test]
    fn compute_ranges_finds_expected_character_classes() {
        let mut table = new_table(3, 256);
        for byte in b'a'..=b'z' {
            table.set_transition(0, byte, 1);
        }
        for byte in b'A'..=b'Z' {
            table.set_transition(0, byte, 1);
        }
        for byte in b'0'..=b'9' {
            table.set_transition(0, byte, 2);
        }

        let state_zero_ranges = table.compute_ranges().remove(0);
        let interesting: Vec<_> = state_zero_ranges
            .into_iter()
            .filter(|(_, _, target)| *target != 0)
            .collect();

        assert_eq!(
            interesting,
            vec![(b'0', b'9', 2), (b'A', b'Z', 1), (b'a', b'z', 1)]
        );
    }

    #[cfg(feature = "regex")]
    #[test]
    fn from_regex_patterns_finds_all_literals() {
        let jit = JitDfa::from_regex_patterns(&["hello", "world"]).unwrap();

        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"say hello to the world", &mut matches);

        assert_eq!(count, 2);
        assert_eq!(matches[0].start, 4);
        assert_eq!(matches[0].end, 9);
        assert_eq!(matches[1].start, 17);
        assert_eq!(matches[1].end, 22);
    }

    #[test]
    fn compute_ranges_detects_alpha_ranges() {
        let mut table = new_table(3, 256);
        for b in b'a'..=b'z' {
            table.set_transition(0, b, 1);
        }
        for b in b'A'..=b'Z' {
            table.set_transition(0, b, 1);
        }
        for b in b'0'..=b'9' {
            table.set_transition(0, b, 2);
        }

        let ranges = table.compute_ranges();
        let state0 = &ranges[0];

        // Should have: [0..47]->0, [48..57]->2, [58..64]->0, [65..90]->1,
        //              [91..96]->0, [97..122]->1, [123..255]->0
        assert_eq!(state0.len(), 7);

        // Verify [a-z] range
        let az = state0.iter().find(|r| r.0 == b'a').unwrap();
        assert_eq!(az, &(b'a', b'z', 1));

        // Verify [A-Z] range
        let big_az = state0.iter().find(|r| r.0 == b'A').unwrap();
        assert_eq!(big_az, &(b'A', b'Z', 1));

        // Verify [0-9] range
        let digits = state0.iter().find(|r| r.0 == b'0').unwrap();
        assert_eq!(digits, &(b'0', b'9', 2));
    }

    #[test]
    fn compute_ranges_all_same_target() {
        let table = new_table(2, 256);
        // All transitions default to 0
        let ranges = table.compute_ranges();
        assert_eq!(ranges[0].len(), 1); // single range [0..255] -> 0
        assert_eq!(ranges[0][0], (0, 255, 0));
    }

    #[test]
    fn compute_ranges_every_byte_different() {
        let mut table = new_table(257, 256);
        for b in 0u16..256 {
            table.set_transition(0, b as u8, b as u32 + 1);
        }
        let ranges = table.compute_ranges();
        assert_eq!(ranges[0].len(), 256); // each byte is its own range
    }

    #[test]
    fn transition_density_single_target() {
        let table = new_table(2, 256);
        // All bytes -> 0, so density = 1
        assert_eq!(table.transition_density(0), 1);
    }

    #[test]
    fn transition_density_alpha_numeric() {
        let mut table = new_table(3, 256);
        for b in b'a'..=b'z' {
            table.set_transition(0, b, 1);
        }
        for b in b'0'..=b'9' {
            table.set_transition(0, b, 2);
        }
        // 3 distinct targets: 0 (default), 1 (alpha), 2 (digit)
        assert_eq!(table.transition_density(0), 3);
    }

    #[test]
    fn minimize_preserves_different_pattern_ids() {
        // States 1 and 2 are equivalent in transitions but accept different patterns.
        // Hopcroft must NOT merge them, or compilation will fail with
        // "multiple accept patterns".
        let mut table = new_table(3, 256);
        for b in 0..=255u8 {
            table.set_transition(0, b, 0);
            table.set_transition(1, b, 0);
            table.set_transition(2, b, 0);
        }
        table.set_transition(0, b'a', 1);
        table.set_transition(0, b'b', 2);
        table.add_accept(1, 0);
        table.add_accept(2, 1);
        table.set_pattern_length(0, 1);
        table.set_pattern_length(1, 1);

        let minimized = table.minimize().unwrap_or(table.clone());
        let jit = JitDfa::compile(&minimized).unwrap();

        let mut matches = vec![Match::from_parts(0, 0, 0); 10];
        let count = jit.scan(b"ab", &mut matches);
        assert_eq!(count, 2);
        assert_eq!(matches[0].pattern_id, 0);
        assert_eq!(matches[1].pattern_id, 1);
    }

    #[test]
    fn minimize_all_accept_same_pattern() {
        let mut table = new_table(2, 256);
        for b in 0..=255u8 {
            table.set_transition(0, b, 0);
            table.set_transition(1, b, 1);
        }
        table.add_accept(0, 0);
        table.add_accept(1, 0);
        table.set_pattern_length(0, 1);

        let minimized = table.minimize().expect("should minimize");
        assert_eq!(minimized.state_count(), 1);

        let jit = JitDfa::compile(&minimized).unwrap();
        assert_eq!(jit.scan_count(b"xxx"), 3);
    }

    #[test]
    fn minimize_all_dead_collapses_to_one() {
        let mut table = new_table(3, 256);
        for state in 0..3 {
            for b in 0..=255u8 {
                table.set_transition(state, b, 0);
            }
        }
        // No accept states

        let minimized = table.minimize().expect("should minimize");
        assert_eq!(minimized.state_count(), 1);

        let jit = JitDfa::compile(&minimized).unwrap();
        assert_eq!(jit.scan_count(b"xxx"), 0);
    }

    #[test]
    fn minimize_single_state_accept() {
        let mut table = new_table(1, 256);
        for b in 0..=255u8 {
            table.set_transition(0, b, 0);
        }
        table.add_accept(0, 0);
        table.set_pattern_length(0, 1);

        assert!(table.minimize().is_none());

        let jit = JitDfa::compile(&table).unwrap();
        assert_eq!(jit.scan_count(b"abc"), 3);
    }

    #[test]
    fn serialization_round_trip_1000_patterns() {
        let mut table = new_table(1001, 256);
        for pid in 0..1000 {
            let byte = (pid % 256) as u8;
            let state = (pid + 1) as u32;
            table.set_transition(0, byte, state);
            table.add_accept(state, pid as u32);
            table.set_pattern_length(pid as u32, 1);
        }

        let bytes = table.to_bytes();
        let restored = TransitionTable::from_bytes(&bytes).unwrap();
        assert_eq!(restored.state_count(), table.state_count());
        assert_eq!(restored.class_count(), table.class_count());
        assert_eq!(restored.transitions(), table.transitions());
        assert_eq!(restored.accept_states(), table.accept_states());
        assert_eq!(restored.pattern_lengths(), table.pattern_lengths());

        let jit_orig = JitDfa::compile(&table).unwrap();
        let jit_restored = JitDfa::compile(&restored).unwrap();

        let input = vec![0u8, 1u8, 2u8, 255u8];
        assert_eq!(
            jit_orig.scan_count(&input),
            jit_restored.scan_count(&input)
        );
    }

    #[test]
    fn jit_interpreted_parity_via_minimization() {
        // Build a large redundant DFA (>4096 states -> interpreted fallback)
        let mut table = new_table(5000, 256);
        for state in 0..5000 {
            for b in 0..=255u8 {
                table.set_transition(state, b, 0);
            }
        }
        table.set_transition(0, b'x', 1);
        table.add_accept(1, 0);
        table.set_pattern_length(0, 1);

        let large_jit = JitDfa::compile(&table).unwrap();
        let minimized = table.minimize().expect("should minimize redundant states");
        assert!(minimized.state_count() <= 4096);

        let small_jit = JitDfa::compile(&minimized).unwrap();

        let inputs: [&[u8]; 5] = [b"", b"x", b"xx", b"abc", b"xxxxxxxxxx"];
        for input in inputs {
            assert_eq!(
                large_jit.scan_count(input),
                small_jit.scan_count(input),
                "JIT/interpreted parity failed for input {:?}",
                input
            );
        }
    }

    #[test]
    fn thread_safety_8_threads_many_patterns() {
        use std::sync::Arc;
        use std::thread;

        let patterns: Vec<Vec<u8>> = (0..100)
            .map(|i| format!("p{:02}", i).into_bytes())
            .collect();
        let pattern_refs: Vec<&[u8]> = patterns.iter().map(|p| p.as_slice()).collect();

        let jit = Arc::new(JitDfa::from_patterns(&pattern_refs).unwrap());

        let mut handles = vec![];
        for i in 0..8 {
            let jit_clone = Arc::clone(&jit);
            handles.push(thread::spawn(move || {
                let input = b"p00 p01 p99 xyz";
                let mut matches = vec![Match::from_parts(0, 0, 0); 10];
                for _ in 0..100 {
                    let count = jit_clone.scan(input, &mut matches);
                    assert_eq!(count, 3, "thread {} mismatch", i);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }
}
