use dfajit::table::TransitionTable;
use dfajit::error::Error;

#[test]
fn test_table_new() {
    let table = TransitionTable::new(5, 256).unwrap();
    assert_eq!(table.state_count(), 5);
    assert_eq!(table.class_count(), 256);
    assert_eq!(table.transitions().len(), 5 * 256);
    assert!(table.accept_states().is_empty());
    assert!(table.pattern_lengths.is_empty());
}

#[test]
fn test_table_set_transition() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.set_transition(0, b'A', 1);
    
    assert_eq!(table.transitions()[b'A' as usize], 1);
    assert_eq!(table.transitions()[b'B' as usize], 0);
}

#[test]
fn test_table_add_accept() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.add_accept(1, 0);
    
    assert_eq!(table.accept_states().len(), 1);
    assert_eq!(table.accept_states()[0], (1, 0));
    assert_eq!(table.pattern_lengths().len(), 1);
    assert_eq!(table.pattern_lengths()[0], 0);
}

#[test]
fn test_table_set_pattern_length() {
    let mut table = TransitionTable::new(2, 256).unwrap();
    table.add_accept(1, 0);
    table.set_pattern_length(0, 5);
    
    assert_eq!(table.pattern_lengths()[0], 5);
}

#[test]
fn test_table_add_accept_resizes_pattern_lengths() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.add_accept(1, 0);
    table.add_accept(2, 5);
    
    assert_eq!(table.pattern_lengths().len(), 6);
    assert_eq!(table.pattern_lengths()[0], 0);
    assert_eq!(table.pattern_lengths()[5], 0);
}

#[test]
fn test_table_from_bytes_empty() {
    let result = TransitionTable::from_bytes(&[]);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_table_serialize_roundtrip() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'A', 1);
    table.set_transition(1, b'B', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);
    
    let bytes = table.to_bytes();
    let decoded = TransitionTable::from_bytes(&bytes).unwrap();
    
    assert_eq!(decoded.state_count(), table.state_count());
    assert_eq!(decoded.class_count(), table.class_count());
    assert_eq!(decoded.transitions(), table.transitions);
    assert_eq!(decoded.accept_states(), table.accept_states);
    assert_eq!(decoded.pattern_lengths(), table.pattern_lengths);
}

#[test]
fn test_table_is_jit_eligible() {
    let table = TransitionTable::new(100, 256).unwrap();
    assert!(table.is_jit_eligible());
    
    let large_table = TransitionTable::new(5000, 256).unwrap();
    assert!(!large_table.is_jit_eligible());
    
    let non_byte_table = TransitionTable::new(100, 128).unwrap();
    assert!(!non_byte_table.is_jit_eligible());
}

#[test]
fn test_table_estimated_code_size() {
    let table = TransitionTable::new(10, 256).unwrap();
    let size = table.estimated_code_size();
    assert!(size > 0);
    assert!(size < 1024 * 1024); // reasonable bounds
}

#[test]
fn test_table_transition_density() {
    let mut table = TransitionTable::new(1, 256).unwrap();
    let mut density = table.transition_density();
    assert_eq!(density, 0.0);
    
    for i in 0..128 {
        table.set_transition(0, i as u8, 1);
    }
    
    density = table.transition_density();
    assert_eq!(density, 0.5);
}
