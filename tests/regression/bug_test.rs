use dfajit::JitDfa;
use matchkit::Match;

#[test]
fn test_missing_failure_transitions() {
    let jit = JitDfa::from_patterns(&[b"foo"]).unwrap_or_else(|_| panic!("failed to build DFA"));
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let count = jit.scan(b"ffoo", &mut matches);
    assert_eq!(count, 1);
}
