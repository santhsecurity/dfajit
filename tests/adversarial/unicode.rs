use dfajit::{JitDfa, Match};
use dfajit::table::TransitionTable;

#[test]
fn test_rtl_and_combining_characters() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, 0xE2, 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    
    let dfa = JitDfa::compile(&table).unwrap();
    // Some arabic text + combining marks.
    let input = "مرحبا بالعالم \u{064B}\u{064B}".as_bytes();
    let mut matches = vec![Match::from_parts(0, 0, 0); 100];
    let count = dfa.scan(&input, &mut matches);
    
    // We aren't testing regex correctness here since this is raw byte DFA,
    // Just testing it doesn't panic on multi-byte unicode.
    assert!(count >= 0);
}

#[test]
fn test_zero_width_joiner_and_emoji() {
    let mut table = TransitionTable::new(5, 256).unwrap();
    table.set_transition(0, 0xF0, 1);
    table.set_transition(1, 0x9F, 2);
    table.set_transition(2, 0x91, 3);
    table.set_transition(3, 0xA8, 4);
    table.add_accept(4, 0);
    table.set_pattern_length(0, 4);
    
    let dfa = JitDfa::compile(&table).unwrap();
    let input = "👨‍👩‍👦".as_bytes();
    let mut matches = vec![Match::from_parts(0, 0, 0); 100];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 1); // Found the 👨
}
