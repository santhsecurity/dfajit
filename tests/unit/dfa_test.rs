use dfajit::{JitDfa, TransitionTable, Error};
use matchkit::Match;

#[test]
fn test_empty_dfa() {
    let table = TransitionTable::new(0, 256).unwrap();
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::EmptyDfa)));
}

#[test]
fn test_dfa_exceeds_states() {
    let table = TransitionTable::new(5000, 256).unwrap();
    let result = JitDfa::compile(&table);
    assert!(result.is_ok()); // Note: currently large DFA falls back to interpreted mode, does not error
}

#[test]
fn test_dfa_from_patterns_empty() {
    let patterns: Vec<&[u8]> = vec![];
    let result = JitDfa::from_patterns(&patterns);
    assert!(matches!(result, Err(Error::EmptyDfa)));
}

#[test]
fn test_dfa_from_patterns_single() {
    let patterns: Vec<&[u8]> = vec![b"abc"];
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = dfa.scan(b"xabcx", &mut matches);
    assert_eq!(count, 1);
    assert_eq!(matches[0].pattern_id, 0);
    assert_eq!(matches[0].start, 1);
    assert_eq!(matches[0].end, 4);
}

#[test]
fn test_dfa_from_patterns_multiple() {
    let patterns: Vec<&[u8]> = vec![b"abc", b"x", b"yz"];
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = dfa.scan(b"xabcyz", &mut matches);
    assert_eq!(count, 3);
    assert_eq!(matches[0].pattern_id, 1); // x
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 1);

    assert_eq!(matches[1].pattern_id, 0); // abc
    assert_eq!(matches[1].start, 1);
    assert_eq!(matches[1].end, 4);

    assert_eq!(matches[2].pattern_id, 2); // yz
    assert_eq!(matches[2].start, 4);
    assert_eq!(matches[2].end, 6);
}

#[test]
fn test_has_match_empty() {
    let patterns: Vec<&[u8]> = vec![b"abc"];
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    
    assert!(!dfa.has_match(b""));
}

#[test]
fn test_has_match_true() {
    let patterns: Vec<&[u8]> = vec![b"abc"];
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    
    assert!(dfa.has_match(b"foo abc bar"));
}

#[test]
fn test_has_match_false() {
    let patterns: Vec<&[u8]> = vec![b"abc"];
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    
    assert!(!dfa.has_match(b"foo abd bar"));
}
