use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

#[test]
fn test_adversarial_empty_input() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let dfa = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    
    // Empty input
    let count = dfa.scan(&[], &mut matches);
    assert_eq!(count, 0, "Scanning empty slice should return 0 matches");
}

#[test]
fn test_adversarial_single_byte_0xff() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, 0xFF, 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let dfa = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    
    // Single 0xFF byte
    let count = dfa.scan(&[0xFF], &mut matches);
    assert_eq!(count, 1, "Failed to match single 0xFF byte");
}

#[test]
fn test_adversarial_alternating_pattern() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.set_transition(1, b'y', 2);
    // Restart logic is handled internally usually, but let's just make accepting
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let dfa = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    
    // Input is max size, repeating "xy"
    let mut input = Vec::with_capacity(1_000_000);
    for _ in 0..500_000 {
        input.push(b'x');
        input.push(b'y');
    }
    
    let count = dfa.scan_count(&input);
    assert_eq!(count, 500_000, "Should match alternating pattern perfectly");
}

#[test]
fn test_adversarial_max_hash_collision_pattern() {
    // Some engines use hashes internally. If we pass purely overlapping states, we check for regressions or infinite loops.
    let mut table = TransitionTable::new(5, 256).unwrap();
    for i in 0..4 {
        for b in 0..=255u8 {
            table.set_transition(i, b, i + 1); // everything goes to next state
        }
    }
    table.add_accept(4, 0);
    table.set_pattern_length(0, 4);

    let dfa = JitDfa::compile(&table).unwrap();
    
    // An input of all identical bytes, triggering max transitions.
    let input = vec![0xAA; 100_000];
    let count = dfa.scan_count(&input);
    
    // Every 4 bytes is a match, then DFA typically resets (based on semantics of engine).
    // If it resets, 100,000 / 4 = 25,000.
    assert_eq!(count, 25_000, "Should handle pure transition overlapping smoothly");
}

#[test]
fn test_adversarial_all_zero_bytes() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, 0x00, 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);

    let dfa = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 256];
    
    let input = vec![0x00; 256];
    let count = dfa.scan(&input, &mut matches);
    
    assert_eq!(count, 256, "Should process null bytes properly");
}
