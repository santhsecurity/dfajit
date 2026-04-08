use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;

#[test]
fn test_compile_scan_extreme_permutations() {
    let mut table = TransitionTable::new(5, 256).unwrap();
    
    // state 0: start. 
    // state 1: saw 'a'
    // state 2: saw 'b'
    // state 3: saw 'c'
    // state 4: sink state / other match
    
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.set_transition(2, b'c', 3);
    table.add_accept(3, 0);
    table.set_pattern_length(0, 3);
    
    table.set_transition(0, b'x', 4);
    table.set_transition(4, b'y', 4);
    table.set_transition(4, b'z', 4);
    table.add_accept(4, 1);
    table.set_pattern_length(1, 1);

    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    
    let input = b"abc x y z abc";
    let count = jit.scan(input, &mut matches);
    
    assert_eq!(count, 5);
    
    assert_eq!(matches[0].start(), 0);
    assert_eq!(matches[0].end(), 3);
    assert_eq!(matches[0].pattern_id(), 0);
    
    assert_eq!(matches[1].start(), 4);
    assert_eq!(matches[1].end(), 5);
    assert_eq!(matches[1].pattern_id(), 1);
    
    assert_eq!(matches[2].start(), 6);
    assert_eq!(matches[2].end(), 7);
    assert_eq!(matches[2].pattern_id(), 1);
    
    assert_eq!(matches[3].start(), 8);
    assert_eq!(matches[3].end(), 9);
    assert_eq!(matches[3].pattern_id(), 1);
    
    assert_eq!(matches[4].start(), 10);
    assert_eq!(matches[4].end(), 13);
    assert_eq!(matches[4].pattern_id(), 0);
}
