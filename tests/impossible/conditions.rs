#![allow(clippy::unwrap_used)]
#![allow(clippy::pedantic)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::expect_used)]
use dfajit::{JitDfa, TransitionTable};
use matchkit::match_type::Match;
use std::thread;

// 1. DFA with 10000 states
#[test]
fn test_dfa_10000_states() {
    let mut table = TransitionTable::new(10000, 256).expect("Fix: should not fail in test");
    for i in 0..9999 {
        table.set_transition(i, b'A', (i + 1) as u32);
    }
    table.add_accept(9999, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = vec![b'A'; 9999];
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 1);
}

// 2. DFA with 256 character classes
#[test]
fn test_dfa_256_classes() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    for i in 0..256 {
        table.set_transition(0, i as u8, 1);
    }
    table.add_accept(1, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = (0..=255).collect::<Vec<u8>>();
    let mut matches = vec![Match::default(); 256];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 256);
}

// 3. DFA scan on 100MB input
#[test]
fn test_dfa_100mb_input() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'X', 1);
    table.set_transition(1, b'X', 1);
    table.add_accept(1, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = vec![b'X'; 100 * 1024 * 1024];
    let mut matches = vec![Match::default(); 1];
    let _ = dfa.scan(&input, &mut matches);
}

// 4. DFA with single accept state at state 0
#[test]
fn test_dfa_accept_at_state_0() {
    let mut table = TransitionTable::new(1, 256).expect("Fix: should not fail in test");
    table.add_accept(0, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"A";
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(input, &mut matches);
    assert_eq!(count, 1);
}

// 5. DFA with ALL states as accept states
#[test]
fn test_dfa_all_states_accept() {
    let mut table = TransitionTable::new(100, 256).expect("Fix: should not fail in test");
    for i in 0..100 {
        if i < 99 {
            table.set_transition(i as usize, b'A', (i + 1) as u32);
        }
        table.add_accept(i as u32, 0);
    }
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = vec![b'A'; 99];
    let mut matches = vec![Match::default(); 100];
    let count = dfa.scan(&input, &mut matches);
    assert!(count > 0);
}

// 6. DFA transition table with maximum u32 entries
#[test]
fn test_dfa_max_u32_entries() {
    let mut table = TransitionTable::new(65536, 256).expect("Fix: should not fail in test");
    table.set_transition(65535, 255, u32::MAX);
    table.add_accept(u32::MAX, 0);
    let _dfa = JitDfa::compile(&table);
}

// 7. Serialization of 50MB DFA
#[test]
fn test_dfa_serialization_50mb() {
    let mut table = TransitionTable::new(65536, 256).expect("Fix: should not fail in test");
    for i in 0..65535 {
        table.set_transition(i, b'X', (i + 1) as u32);
    }
    table.add_accept(65535, 0);
    let serialized = table.to_bytes();
    assert!(serialized.len() > 1024 * 1024);
    let table2 = TransitionTable::from_bytes(&serialized).expect("Fix: should not fail in test");
    assert_eq!(table.state_count(), table2.state_count());
}

// 8. Deserialization from corrupted bytes
#[test]
fn test_dfa_corrupted_bytes() {
    let corrupted_bytes = vec![0x00, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let res = TransitionTable::from_bytes(&corrupted_bytes);
    assert!(res.is_err());
}

// 9. JIT scan parity with interpreted scan on 1000 random patterns
#[test]
fn test_dfa_jit_parity() {
    let mut table = TransitionTable::new(3, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'B', 2);
    table.add_accept(2, 0);
    // Add default transitions to 0 to simulate full table
    for i in 0..=255 {
        if i != b'A' {
            table.set_transition(0, i, 0);
        }
        if i != b'B' && i != b'A' {
            table.set_transition(1, i, 0);
        }
        if i == b'A' {
            table.set_transition(1, i, 1);
        }
        if i != b'A' {
            table.set_transition(2, i, 0);
        }
        if i == b'A' {
            table.set_transition(2, i, 1);
        }
    }
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");

    // Test parity on 1000 randomly generated sequences
    use rand::{RngExt, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    for _ in 0..1000 {
        let len = rng.random_range(1..100);
        let mut input = vec![0u8; len];
        rng.fill(&mut input[..]);

        // Scan with JIT
        let mut matches_jit = vec![Match::default(); 100];
        let count_jit = dfa.scan(&input, &mut matches_jit);

        // Interpreted scan parity directly replicating the table logic
        let mut true_interpreted = 0;
        let mut state: usize = 0;
        let transitions = table.transitions();
        let accepts = table.accept_states();

        for i in 0..input.len() {
            let offset = (state * 256) + input[i] as usize;
            state = transitions[offset] as usize;

            // Check if current state is an accept state
            for &(acc_state, _) in accepts {
                if acc_state as usize == state {
                    true_interpreted += 1;
                    break; // Only count once per position
                }
            }
        }

        assert_eq!(count_jit, true_interpreted, "Mismatch on input {:?}", input);
    }
}

// 10. Concurrent JIT compilation from 4 threads
#[test]
fn test_dfa_concurrent_compilation() {
    let mut handles = vec![];
    for _ in 0..4 {
        handles.push(thread::spawn(|| {
            let mut table = TransitionTable::new(10, 256).expect("Fix: should not fail in test");
            table.set_transition(0, b'X', 1);
            table.add_accept(1, 0);
            let _dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
        }));
    }
    for h in handles {
        h.join().expect("Fix: should not fail in test");
    }
}

// 11. DFA zero states
#[test]
fn test_dfa_zero_states() {
    let res = TransitionTable::new(0, 256);
    if res.is_ok() {
        let table = res.expect("Fix: should not fail in test");
        let compile_res = JitDfa::compile(&table);
        assert!(compile_res.is_err());
    } else {
        assert!(res.is_err());
    }
}

// 12. DFA zero classes
#[test]
fn test_dfa_zero_classes() {
    let res = TransitionTable::new(10, 0);
    assert!(res.is_err());
}

// 13. DFA too many states
#[test]
fn test_dfa_too_many_states() {
    let res = TransitionTable::new(65537, 256);
    assert!(res.is_err());
}

// 14. DFA out of bounds transition
#[test]
fn test_dfa_out_of_bounds_transition() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 2);
    let _dfa = JitDfa::compile(&table);
}

// 15. DFA self loop all chars
#[test]
fn test_dfa_self_loop_all_chars() {
    let mut table = TransitionTable::new(1, 256).expect("Fix: should not fail in test");
    for i in 0..256 {
        table.set_transition(0, i as u8, 0);
    }
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = vec![b'X'; 1000];
    let mut matches = vec![];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 0);
}

// 16. DFA empty input
#[test]
fn test_dfa_empty_input() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(b"", &mut matches);
    assert_eq!(count, 0);
}

// 17. DFA multiple accepts same state
#[test]
fn test_dfa_multiple_accepts_same_state() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.add_accept(1, 0);
    table.add_accept(1, 1);
    table.add_accept(1, 2);
    let compile_res = JitDfa::compile(&table);
    assert!(compile_res.is_err());
}

// 18. DFA circular transitions
#[test]
fn test_dfa_circular_transitions() {
    let mut table = TransitionTable::new(3, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'B', 2);
    table.set_transition(2, b'C', 0);
    table.add_accept(0, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"ABCABCABC";
    let mut matches = vec![Match::default(); 10];
    let count = dfa.scan(input, &mut matches);
    assert!(count > 0);
}

// 19. DFA unreachable accept state
#[test]
fn test_dfa_unreachable_accept_state() {
    let mut table = TransitionTable::new(3, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.add_accept(2, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"AAABBBCCC";
    let mut matches = vec![];
    let count = dfa.scan(input, &mut matches);
    assert_eq!(count, 0);
}

// 20. DFA all transitions to sink
#[test]
fn test_dfa_all_transitions_to_sink() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    for i in 0..256 {
        table.set_transition(0, i as u8, 1);
        table.set_transition(1, i as u8, 1);
    }
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = vec![0; 1000];
    let mut matches = vec![];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 0);
}

// 21. DFA alternating accept states
#[test]
fn test_dfa_alternating_accept_states() {
    let mut table = TransitionTable::new(4, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'B', 2);
    table.set_transition(2, b'A', 3);
    table.set_transition(3, b'B', 0);
    table.add_accept(1, 0);
    table.add_accept(3, 1);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"ABABABAB";
    let mut matches = vec![Match::default(); 8];
    let count = dfa.scan(input, &mut matches);
    assert!(count >= 4);
}

// 22. DFA match array too small
#[test]
fn test_dfa_match_array_too_small() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'A', 1);
    table.add_accept(1, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"AAAAA";
    let mut matches = vec![Match::default(); 2];
    let count = dfa.scan(input, &mut matches);
    assert!(count >= 2);
}

// 23. DFA overlapping patterns
#[test]
fn test_dfa_overlapping_patterns() {
    let mut table = TransitionTable::new(4, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'A', 2);
    table.set_transition(2, b'A', 3);
    table.set_transition(3, b'A', 3);
    table.add_accept(1, 0);
    table.add_accept(2, 1);
    table.add_accept(3, 2);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"AAAA";
    let mut matches = vec![Match::default(); 10];
    let count = dfa.scan(input, &mut matches);
    assert!(count > 0);
}

// 24. DFA incomplete pattern
#[test]
fn test_dfa_incomplete_pattern() {
    let mut table = TransitionTable::new(3, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'B', 2);
    table.add_accept(2, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(b"A", &mut matches);
    assert_eq!(count, 0);
}

// 25. DFA start state accept
#[test]
fn test_dfa_start_state_accept() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.add_accept(0, 0);
    table.set_transition(0, b'X', 1);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(b"X", &mut matches);
    assert_eq!(count, 0);
}

// 26. DFA non ascii transitions
#[test]
fn test_dfa_non_ascii_transitions() {
    let mut table = TransitionTable::new(3, 256).expect("Fix: should not fail in test");
    table.set_transition(0, 0xFF, 1);
    table.set_transition(1, 0xFE, 2);
    table.add_accept(2, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = [0xFF, 0xFE];
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 1);
}

// 27. DFA null byte transitions
#[test]
fn test_dfa_null_byte_transitions() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, 0x00, 1);
    table.add_accept(1, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = [0x00, 0x00, 0x00];
    let mut matches = vec![Match::default(); 3];
    let count = dfa.scan(&input, &mut matches);
    assert!(count > 0);
}

// 28. DFA concurrent scans
#[test]
fn test_dfa_concurrent_scans() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'X', 1);
    table.add_accept(1, 0);
    let dfa = std::sync::Arc::new(JitDfa::compile(&table).expect("Fix: should not fail in test"));

    let mut handles = vec![];
    for _ in 0..4 {
        let dfa_clone = dfa.clone();
        handles.push(thread::spawn(move || {
            let input = b"XXXXX";
            let mut matches = vec![Match::default(); 5];
            let count = dfa_clone.scan(input, &mut matches);
            assert_eq!(count, 5);
        }));
    }
    for h in handles {
        h.join().expect("Fix: should not fail in test");
    }
}

// 29. DFA invalid deserialization length
#[test]
fn test_dfa_invalid_deserialization_length() {
    let table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    let mut bytes = table.to_bytes();
    bytes.truncate(bytes.len() - 1);
    let res = TransitionTable::from_bytes(&bytes);
    assert!(res.is_err());
}

// 30. DFA invalid deserialization magic
#[test]
fn test_dfa_invalid_deserialization_magic() {
    let table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    let mut bytes = table.to_bytes();
    bytes[0] ^= 0xFF;
    let res = TransitionTable::from_bytes(&bytes);
    assert!(res.is_err());
}

// 31. DFA huge match array
#[test]
fn test_dfa_huge_match_array() {
    let mut table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"A";
    let mut matches = vec![Match::default(); 100_000];
    let count = dfa.scan(input, &mut matches);
    assert_eq!(count, 1);
}

// 32. DFA deserialize zero state hack
#[test]
fn test_dfa_deserialize_zero_state() {
    let table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    let mut bytes = table.to_bytes();
    if bytes.len() >= 8 {
        bytes[4] = 0;
        bytes[5] = 0;
        bytes[6] = 0;
        bytes[7] = 0;
    }
    let res = TransitionTable::from_bytes(&bytes);
    let _ = res;
}

// 33. DFA match after transition out of accept state
#[test]
fn test_dfa_match_after_transition_out() {
    let mut table = TransitionTable::new(3, 256).expect("Fix: should not fail in test");
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    table.set_transition(1, b'B', 2);
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(b"AB", &mut matches);
    assert!(count > 0);
}

// 34. DFA missing initial class class transition
#[test]
fn test_dfa_missing_initial() {
    let table = TransitionTable::new(2, 256).expect("Fix: should not fail in test");
    let dfa = JitDfa::compile(&table).expect("Fix: should not fail in test");
    let input = b"";
    let mut matches = vec![Match::default(); 1];
    let count = dfa.scan(input, &mut matches);
    assert_eq!(count, 0);
}
