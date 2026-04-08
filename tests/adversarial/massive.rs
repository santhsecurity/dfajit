use dfajit::{JitDfa, Match};
use dfajit::table::TransitionTable;

#[test]
fn test_extremely_long_pattern() {
    let mut table = TransitionTable::new(1001, 256).unwrap();
    for i in 0..1000 {
        table.set_transition(i, b'x', (i + 1) as u32);
    }
    table.add_accept(1000, 0);
    table.set_pattern_length(0, 1000);

    let dfa = JitDfa::compile(&table).unwrap();
    
    // Exact match
    let mut input = vec![b'x'; 1000];
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 1);
    assert_eq!(matches[0].start(), 0);
    assert_eq!(matches[0].end(), 1000);

    // Overlapping matches
    input.push(b'x');
    let count = dfa.scan(&input, &mut matches);
    // DFA resets on match, so it matches the first 1000 'x's, then has 1 'x' left.
    assert_eq!(count, 1);
    assert_eq!(matches[0].start(), 0);
    assert_eq!(matches[0].end(), 1000);
}

#[test]
fn test_massive_table_limits() {
    // Note: the code states 4096 is the limit.
    // Ensure that it handles something very close to the limit without failing compilation.
    let mut table = TransitionTable::new(4090, 256).unwrap();
    table.set_transition(0, b'A', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    let dfa = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = dfa.scan(b"A", &mut matches);
    assert_eq!(count, 1);
}

#[test]
fn test_all_possible_bytes() {
    let mut table = TransitionTable::new(257, 256).unwrap();
    for i in 0..=255 {
        table.set_transition(0, i as u8, (i + 1) as u32);
        table.add_accept((i + 1) as u32, i as u32);
        table.set_pattern_length(i as u32, 1);
    }
    
    let dfa = JitDfa::compile(&table).unwrap();
    
    let mut input: Vec<u8> = (0..=255).collect();
    let mut matches = vec![Match::from_parts(0, 0, 0); 300];
    
    let count = dfa.scan(&input, &mut matches);
    assert_eq!(count, 256);
    
    for i in 0..256 {
        assert_eq!(matches[i].pattern_id(), i as u32);
        assert_eq!(matches[i].start(), i);
        assert_eq!(matches[i].end(), i + 1);
    }
}
