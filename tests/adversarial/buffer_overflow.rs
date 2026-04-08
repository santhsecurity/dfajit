#![allow(clippy::unwrap_used, clippy::panic)]

//! Buffer Overflow Prevention Tests
//!
//! These tests verify that the DFA implementation properly handles:
//! 1. State count overflow in transition table indexing
//! 2. Pattern ID bounds checking
//! 3. Serialization/deserialization overflow protection
//! 4. Multiplication overflow in table size calculations

use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

/// Test that overflow in state_count * class_count is caught.
#[test]
#[cfg(debug_assertions)]
fn test_new_table_overflow_panics_in_debug() {
    let huge = usize::MAX;
    // This should panic in debug mode due to checked_mul
    let result = std::panic::catch_unwind(|| {
        let _ = TransitionTable::new(huge, 2).unwrap();
    });
    assert!(result.is_err(), "Should panic on overflow in debug mode");
}

/// Test that reasonable large tables don't panic.
#[test]
fn test_large_but_valid_table() {
    // 10000 * 256 = 2,560,000 transitions - large but valid
    let table = TransitionTable::new(10000, 256).unwrap();
    assert_eq!(table.transitions().len(), 2_560_000);
}

/// Test pattern ID bounds in accept states.
#[test]
fn test_pattern_id_bounds_in_accept_states() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    // Pattern 0 has length defined, but let's try to reference undefined pattern

    // This should work since we defined pattern 0
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"x", &mut matches);
    assert_eq!(count, 1);
}

/// Test that missing pattern length is caught during compile.
#[test]
fn test_missing_pattern_length_caught() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    // Add accept state but don't set pattern length
    table.accept_states_mut().push((1, 0));
    // pattern_lengths is empty, so pattern 0 has no length defined

    let result = JitDfa::compile(&table);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("has no length defined"));
}

/// Test overflow protection in from_bytes.
#[test]
fn test_from_bytes_overflow_protection() {
    // Craft a header that would cause overflow
    let mut bytes = vec![0u8; 8];
    // state_count = u32::MAX
    bytes[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    // class_count = 2
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());

    let result = TransitionTable::from_bytes(&bytes);
    assert!(
        result.is_err(),
        "Should reject overflow-inducing dimensions"
    );
}

/// Test that state index overflow in transitions is caught.
#[test]
fn test_transition_state_index_overflow() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // Set valid transition
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    // Corrupt to have state index that would overflow
    table.transitions_mut()[0] = u32::MAX;

    let result = JitDfa::compile(&table);
    assert!(result.is_err());
}

/// Test serialization round-trip with maximum values.
#[test]
fn test_serialization_max_values() {
    // Use large but valid values
    let table = TransitionTable::new(1000, 256).unwrap();

    let bytes = table.to_bytes();
    let restored = TransitionTable::from_bytes(&bytes).unwrap();

    assert_eq!(restored.state_count(), table.state_count());
    assert_eq!(restored.class_count(), table.class_count());
}

/// Test that compute_ranges handles edge cases safely.
#[test]
fn test_compute_ranges_edge_cases() {
    // Empty DFA - no states means no ranges
    let table = TransitionTable::new(0, 256).unwrap();
    let ranges = table.compute_ranges();
    assert!(ranges.is_empty());

    // Single state with zero classes
    let table_err = TransitionTable::new(1, 0);
    assert!(table_err.is_err());
}

/// Test minimize handles edge cases safely.
#[test]
fn test_minimize_edge_cases() {
    // Empty table
    let table = TransitionTable::new(0, 256).unwrap();
    let minimized = table.minimize();
    assert!(minimized.is_none());

    // Single state
    let table = TransitionTable::new(1, 256).unwrap();
    let minimized = table.minimize();
    assert!(minimized.is_none());
}

/// Test that corrupted transitions are detected.
#[test]
fn test_corrupted_transition_detection() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    // Corrupt transition to point outside state count
    table.transitions_mut()[0] = 100;

    let result = JitDfa::compile(&table);
    assert!(result.is_err());
}

/// Test that accept state pointing to non-existent state is rejected.
#[test]
fn test_invalid_accept_state_rejected() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // Accept state 999 doesn't exist
    table.accept_states_mut().push((999, 0));
    table.pattern_lengths_mut().push(1);

    let result = JitDfa::compile(&table);
    assert!(result.is_err());
}

/// Test estimated_code_size doesn't overflow.
#[test]
fn test_estimated_code_size_no_overflow() {
    let table = TransitionTable::new(10000, 256).unwrap();
    let size = table.estimated_code_size();
    // Should be a reasonable value, not overflowed
    assert!(size > 0);
    assert!(size < usize::MAX / 2);
}

/// Test that pattern_lengths resize works correctly.
#[test]
fn test_pattern_lengths_resize() {
    let mut table = TransitionTable::new(2, 256).unwrap();

    // Add accept for pattern 5 without intermediate patterns
    table.add_accept(1, 5);

    // pattern_lengths should be resized to accommodate index 5
    assert!(table.pattern_lengths().len() > 5);

    // Set the length
    table.set_pattern_length(5, 10);
    assert_eq!(table.pattern_lengths()[5], 10);
}

/// Test from_bytes with truncated pattern lengths section.
#[test]
fn test_from_bytes_truncated_pattern_lengths() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let bytes = table.to_bytes();

    // Truncate to remove part of pattern lengths
    for truncate_at in 1..=4 {
        if bytes.len() > truncate_at {
            let truncated = &bytes[..bytes.len() - truncate_at];
            let result = TransitionTable::from_bytes(truncated);
            assert!(
                result.is_err(),
                "Should fail when truncated by {}",
                truncate_at
            );
        }
    }
}

/// Test that DFA state count limit is enforced.
#[test]
fn test_dfa_state_count_limit() {
    // 65537 exceeds MAX_STATES — TransitionTable::new rejects it.
    let result = TransitionTable::new(65537, 256);
    assert!(result.is_err(), "65537 states must be rejected");
}

/// Test that JIT eligibility boundary is respected.
#[test]
fn test_jit_eligibility_boundary() {
    // 4096 states = JIT eligible
    let table_4096 = TransitionTable::new(4096, 256).unwrap();
    assert!(table_4096.is_jit_eligible());

    // 4097 states = not JIT eligible (falls back to interpreted)
    let table_4097 = TransitionTable::new(4097, 256).unwrap();
    assert!(!table_4097.is_jit_eligible());
}

/// Test transition_density with out-of-bounds state.
#[test]
fn test_transition_density_out_of_bounds() {
    let table = TransitionTable::new(2, 256).unwrap();
    let density = table.transition_density(100);
    assert_eq!(density, 0);
}

/// Test set_transition with out-of-bounds state (release mode behavior).
#[test]
#[cfg(not(debug_assertions))]
fn test_set_transition_out_of_bounds_release() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // In release mode, this silently does nothing
    table.set_transition(100, b'x', 1);

    // Verify table is still valid
    assert_eq!(table.transitions_mut()[0], 0);
}

/// Test set_transition with out-of-bounds state (debug mode behavior).
#[test]
#[cfg(debug_assertions)]
fn test_set_transition_out_of_bounds_debug() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // In debug mode, this panics
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.set_transition(100, b'x', 1);
    }));
    assert!(result.is_err());
}

/// Test from_bytes with forged accept_count causing overflow.
#[test]
fn test_from_bytes_forged_accept_count() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'x', 1);

    let mut bytes = table.to_bytes();

    // Find and corrupt accept_count (after transitions)
    let trans_len = 2 * 256 * 4; // state_count * class_count * 4 bytes
    let accept_count_offset = 8 + trans_len;

    if bytes.len() > accept_count_offset + 4 {
        // Set accept_count to huge value
        bytes[accept_count_offset..accept_count_offset + 4]
            .copy_from_slice(&u32::MAX.to_le_bytes());

        let result = TransitionTable::from_bytes(&bytes);
        assert!(result.is_err());
    }
}
