use dfajit::{Error, JitDfa};

#[test]
fn test_dfa_generation_empty_patterns() {
    let result = JitDfa::from_patterns(&[]);
    assert!(matches!(result, Err(Error::EmptyDfa)));
}

#[test]
fn test_dfa_generation_all_empty_patterns() {
    let result = JitDfa::from_patterns(&[b"", b""]);
    // Since we ignore empty patterns, if all are empty, the states are minimal.
    // However, it technically succeeds. Wait, let's verify what happens.
    // Since pattern.is_empty() does `continue;`, no patterns are processed.
    // The table only has state 0.
    // Wait, let's see. `table.state_count() == 1`. It will compile a DFA that accepts nothing.
    assert!(result.is_ok());
    let dfa = result.unwrap();
    assert_eq!(dfa.state_count(), 1);
}

#[test]
fn test_dfa_generation_extreme_patterns() {
    let mut patterns = Vec::new();
    let pat = b"a";
    // Many duplicates
    for _ in 0..10000 {
        patterns.push(&pat[..]);
    }
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    assert!(dfa.state_count() < 100); // the states should collapse heavily
}

#[test]
fn test_dfa_generation_nested_states() {
    let mut patterns = Vec::new();
    let pat1 = vec![b'a'; 4000]; // Creates 4001 states
    patterns.push(&pat1[..]);
    
    // Should fallback to interpreted or handle it without panic.
    let dfa = JitDfa::from_patterns(&patterns).unwrap();
    assert!(dfa.state_count() >= 4000);
}

#[cfg(feature = "regex")]
#[test]
fn test_regex_generation_unsupported_complex() {
    // Highly nested / explosive regexes might fail memory limits in regex-automata,
    // but should be caught and returned as an Error, not panic.
    let result = JitDfa::from_regex_patterns(&["a{1000}"]);
    // Can succeed or fail gracefully, but not panic.
    assert!(result.is_ok() || matches!(result, Err(Error::InvalidTable { .. })));
    if let Ok(dfa) = result {
        assert!(dfa.state_count() > 0);
    }
}
