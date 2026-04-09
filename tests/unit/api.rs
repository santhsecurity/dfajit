use dfajit::{JitDfa, Match};
use dfajit::table::TransitionTable;

#[test]
fn test_jitdfa_constructor() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    
    let dfa = JitDfa::compile(&table).expect("Failed to compile valid DFA");
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = dfa.scan(b"A", &mut matches);
    assert_eq!(count, 1, "Compiled DFA should match the pattern");
}
