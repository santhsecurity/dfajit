use dfajit::{Error, JitDfa, TransitionTable};

#[test]
fn test_error_empty_dfa() {
    let result = JitDfa::compile(&TransitionTable::new(0, 256));
    assert!(matches!(result, Err(Error::EmptyDfa)));
    
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), "DFA has zero states. Fix: provide at least one state in the transition table.");
}

#[test]
fn test_error_too_many_states() {
    let table = TransitionTable::new(100_000, 256).unwrap();
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::TooManyStates { states: 100_000, max: 65_536 })));
    
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), "DFA has 100000 states, exceeding the 65536-state JIT limit. Fix: use the interpreted fallback for large DFAs.");
}

#[test]
fn test_error_invalid_table_mismatch() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.transitions_mut().push(0); // Add extra
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
    
    let err = result.unwrap_err();
    assert!(err.to_string().contains("transition table has 513 entries but state_count=2 * class_count=256 = 512"));
}

#[test]
fn test_error_invalid_table_overflow() {
    let table = TransitionTable::new(usize::MAX, 2).unwrap();
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_error_invalid_table_out_of_bounds_target() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.transitions_mut()[0] = 5; // Target state 5, but state count is 2
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_error_invalid_table_out_of_bounds_accept() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.add_accept(5, 0); // State 5 is accept, but state count is 2
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_error_invalid_table_duplicate_accept() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.add_accept(1, 0);
    table.add_accept(1, 1);
    // Needs pattern lengths
    table.set_pattern_length(0, 1);
    table.set_pattern_length(1, 1);
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_error_invalid_table_missing_pattern_length() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    // Directly add accept state to skip `add_accept`'s auto-resize of pattern_lengths
    table.accept_states_mut().push((1, 0)); 
    let result = JitDfa::compile(&table);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}
