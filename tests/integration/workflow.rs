#![allow(clippy::unwrap_used, clippy::panic)]

use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

#[test]
fn test_end_to_end_workflow() {
    // 1. Build a table explicitly mimicking a standard multi-pattern match setup.
    let mut table = TransitionTable::new(6, 256).unwrap();
    // Patterns: "cat", "bat", "car"

    // Default transitions to state 0
    for state in 0..6 {
        for b in 0..=255u8 {
            table.set_transition(state, b, 0);
        }
    }

    table.set_transition(0, b'c', 1);
    table.set_transition(0, b'b', 2);

    table.set_transition(1, b'a', 3);
    table.set_transition(2, b'a', 4);

    table.set_transition(3, b't', 5); // cat
    table.set_transition(3, b'r', 5); // car
    table.set_transition(4, b't', 5); // bat

    table.add_accept(5, 0); // They all go to one accept state for simplicity, reporting pat id 0
    table.set_pattern_length(0, 3);

    // 2. Minimize the table (may not actually reduce in this particular contrived case)
    let minimized = table.minimize().unwrap_or(table.clone());

    // 3. Serialize and deserialize
    let bytes = minimized.to_bytes();
    let restored = TransitionTable::from_bytes(&bytes).unwrap();

    // 4. Compile to JIT
    let jit = JitDfa::compile(&restored).unwrap();

    // 5. Scan a realistic buffer
    let input = b"I have a cat and a bat in my car";
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];

    let count = jit.scan(input, &mut matches);
    assert_eq!(count, 3);

    // Verify first match "cat"
    assert_eq!(matches[0].start, 9);
    assert_eq!(matches[0].end, 12);

    // Verify second match "bat"
    assert_eq!(matches[1].start, 19);
    assert_eq!(matches[1].end, 22);

    // Verify third match "car"
    assert_eq!(matches[2].start, 29);
    assert_eq!(matches[2].end, 32);
}

#[test]
fn test_builder_workflow() {
    // End-to-end utilizing the simplified builder API
    let patterns: &[&[u8]] = &[b"sec", b"ret"];
    let jit = JitDfa::from_patterns(patterns).unwrap();

    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let input = b"find the secret token";
    let count = jit.scan(input, &mut matches);

    // "sec" and "ret" will both trigger matches sequentially
    assert_eq!(count, 2);
    assert_eq!(matches[0].pattern_id, 0); // sec
    assert_eq!(matches[1].pattern_id, 1); // ret
    assert_eq!(matches[0].start, 9);
    assert_eq!(matches[1].start, 12);
}
