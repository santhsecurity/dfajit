use dfajit::{JitDfa, Match};
use dfajit::table::TransitionTable;

#[test]
fn test_extremely_large_input_buffer() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    
    let dfa = JitDfa::compile(&table).unwrap();
    
    // Allocate a large buffer.
    // 50MB of data to ensure it correctly scans large continuous regions.
    let buffer = vec![b'B'; 50 * 1024 * 1024];
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = dfa.scan(&buffer, &mut matches);
    assert_eq!(count, 0);
}

#[test]
fn test_frequent_restarts() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);
    
    let dfa = JitDfa::compile(&table).unwrap();
    
    // A string composed entirely of repeated pattern: abababababab
    // This will force the DFA to constantly hit an accept state, resetting to state 0.
    let mut input = Vec::new();
    for _ in 0..1_000_000 {
        input.push(b'a');
        input.push(b'b');
    }
    
    let mut matches = vec![Match::from_parts(0, 0, 0); 2_000_000];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 1_000_000);
}
