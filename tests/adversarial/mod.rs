//! Adversarial tests for `dfajit`.

use std::error::Error;
use std::sync::Arc;
use std::thread;

use dfajit::{JitDfa, TransitionTable};

type TestResult = Result<(), Box<dyn Error>>;

fn reset_table(state_count: usize) -> TransitionTable {
    let mut table = TransitionTable::new(state_count, 256).unwrap();
    for state in 0..state_count {
        for byte in u8::MIN..=u8::MAX {
            table.set_transition(state, byte, 0);
        }
    }
    table
}

#[test]
fn rejects_empty_dfa() {
    let table = TransitionTable::new(0, 256).unwrap();
    assert!(JitDfa::compile(&table).is_err());
}

#[test]
fn supports_maximum_jit_state_count() -> TestResult {
    let mut table = reset_table(4096);
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table)?;
    assert_eq!(jit.state_count(), 4096);
    assert_eq!(jit.scan_count(b"a"), 1);
    Ok(())
}

#[test]
fn supports_large_interpreted_fallback_state_count() -> TestResult {
    let mut table = reset_table(65_536);
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table)?;
    assert_eq!(jit.state_count(), 65_536);
    assert_eq!(jit.scan_count(b"a"), 1);
    Ok(())
}

#[test]
fn overflow_state_count_is_rejected() {
    let table = TransitionTable::new(65_537, 256).unwrap();
    assert!(JitDfa::compile(&table).is_err());
}

#[test]
fn single_byte_input_produces_expected_match() -> TestResult {
    let mut table = reset_table(2);
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table)?;
    let mut matches = Vec::new();
    let count = jit.scan(b"x", &mut matches);

    assert_eq!(count, 1);
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 1);
    Ok(())
}

#[test]
fn all_same_byte_input_is_stable() -> TestResult {
    let mut table = reset_table(2);
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table)?;
    let input = vec![b'a'; 128 * 1024];
    assert_eq!(jit.scan_count(&input), input.len());
    Ok(())
}

#[test]
fn all_same_byte_input_without_accepts_stays_quiet() -> TestResult {
    let table = reset_table(2);
    let jit = JitDfa::compile(&table)?;
    let input = vec![b'a'; 64 * 1024];
    assert_eq!(jit.scan_count(&input), 0);
    Ok(())
}

#[test]
fn alternating_patterns_match_every_position() -> TestResult {
    let mut table = reset_table(3);
    table.set_transition(0, b'a', 1);
    table.set_transition(0, b'b', 2);
    table.set_transition(1, b'b', 2);
    table.set_transition(2, b'a', 1);
    table.add_accept(1, 0);
    table.add_accept(2, 1);
    table.set_pattern_length(0, 1);
    table.set_pattern_length(1, 1);

    let jit = JitDfa::compile(&table)?;
    assert_eq!(jit.scan_count(b"abababab"), 8);
    Ok(())
}

#[test]
fn nul_bytes_are_treated_as_regular_input() -> TestResult {
    let mut table = reset_table(2);
    table.set_transition(0, 0, 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table)?;
    assert_eq!(jit.scan_count(&[0, 1, 0, 0]), 3);
    Ok(())
}

#[test]
fn all_byte_values_remain_matchable() -> TestResult {
    let mut table = reset_table(2);
    for byte in u8::MIN..=u8::MAX {
        table.set_transition(0, byte, 1);
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table)?;
    let input: Vec<u8> = (u8::MIN..=u8::MAX).collect();
    assert_eq!(jit.scan_count(&input), input.len());
    Ok(())
}

#[test]
fn concurrent_scans_share_compiled_program_safely() -> TestResult {
    let mut table = reset_table(3);
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let jit = Arc::new(JitDfa::compile(&table)?);
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let jit = Arc::clone(&jit);
            thread::spawn(move || jit.scan_count(b"zzabzzab"))
        })
        .collect();

    for worker in workers {
        let count = worker
            .join()
            .map_err(|_| std::io::Error::other("worker thread panicked"))?;
        assert_eq!(count, 2);
    }
    Ok(())
}

#[test]
fn zero_length_pattern_offsets_stay_valid() -> TestResult {
    let mut table = reset_table(2);
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 0);

    let jit = JitDfa::compile(&table)?;
    let mut matches = Vec::new();
    jit.scan(b"a", &mut matches);
    assert_eq!(matches[0].start, matches[0].end);
    Ok(())
}

#[test]
fn oversized_pattern_length_saturates_start_offset() -> TestResult {
    let mut table = reset_table(2);
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, u32::MAX);

    let jit = JitDfa::compile(&table)?;
    let mut matches = Vec::new();
    jit.scan(b"a", &mut matches);
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 1);
    Ok(())
}

#[test]
fn compute_ranges_merges_consecutive_bytes_with_shared_target() {
    let mut table = reset_table(3);
    for byte in b'a'..=b'f' {
        table.set_transition(0, byte, 1);
    }
    for byte in b'x'..=b'z' {
        table.set_transition(0, byte, 2);
    }

    let ranges = table.compute_ranges();

    assert!(ranges[0].contains(&(b'a', b'f', 1)));
    assert!(ranges[0].contains(&(b'x', b'z', 2)));
}

#[test]
fn scan_1mb_all_matches() {
    // Every byte matches — worst case for match output
    let mut table = TransitionTable::new(2, 256).unwrap();
    for b in 0..=255u8 {
        table.set_transition(0, b, 1);
        table.set_transition(1, b, 1);
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![b'x'; 1_000_000];
    let mut matches = Vec::new();
    let count = jit.scan(&input, &mut matches);
    assert_eq!(count, 1_000_000);
}

#[test]
fn compile_4096_state_dfa() {
    // Maximum JIT-eligible DFA
    let mut table = TransitionTable::new(4096, 256).unwrap();
    for s in 0..4096 {
        for b in 0..=255u8 {
            table.set_transition(s, b, ((s + 1) % 4096) as u32);
        }
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![matchkit::Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"test", &mut matches);
    assert_eq!(count, 1);
}

#[test]
fn scan_with_binary_data() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, 0x00, 1);
    table.set_transition(1, 0xFF, 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![0x00, 0xFF, 0x00, 0xFF, 0x42];
    let mut matches = Vec::new();
    let count = jit.scan(&input, &mut matches);
    assert_eq!(count, 2);
}
