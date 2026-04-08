use dfajit::{Error, TransitionTable};

#[test]
fn test_from_bytes_empty_data() {
    let result = TransitionTable::from_bytes(&[]);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_truncated_header() {
    let result = TransitionTable::from_bytes(&[1, 0, 0, 0, 1, 0, 0]); // 7 bytes
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_integer_overflow_states_classes() {
    // state_count * class_count > usize::MAX
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&(u32::MAX).to_le_bytes()); // state_count = MAX
    data[4..8].copy_from_slice(&(u32::MAX).to_le_bytes()); // class_count = MAX
    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_byte_length_overflow() {
    // trans_len * 4 overflows
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&(1 << 30).to_le_bytes()); // state_count
    data[4..8].copy_from_slice(&2u32.to_le_bytes()); // class_count
    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_truncated_transition_table() {
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&2u32.to_le_bytes());
    data[4..8].copy_from_slice(&2u32.to_le_bytes());
    // Needs 16 bytes for transitions, we only provide 8
    data.extend_from_slice(&[0; 8]);
    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_truncated_accept_states() {
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..8].copy_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]); // 1 transition (state 0 -> 0)
    
    // Accept count = 1
    data.extend_from_slice(&1u32.to_le_bytes());
    // Only 4 bytes of accept states provided (need 8)
    data.extend_from_slice(&[0; 4]);

    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_accept_states_overflow() {
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..8].copy_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]); // 1 transition (state 0 -> 0)
    
    // Accept count = u32::MAX
    data.extend_from_slice(&(u32::MAX).to_le_bytes());

    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_truncated_pattern_lengths() {
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..8].copy_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]); // 1 transition (state 0 -> 0)
    
    // Accept count = 0
    data.extend_from_slice(&0u32.to_le_bytes());

    // Pat length count = 1
    data.extend_from_slice(&1u32.to_le_bytes());
    // No pattern lengths provided (need 4 bytes)

    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_pattern_lengths_overflow() {
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..8].copy_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]); // 1 transition (state 0 -> 0)
    
    // Accept count = 0
    data.extend_from_slice(&0u32.to_le_bytes());

    // Pat length count = u32::MAX
    data.extend_from_slice(&(u32::MAX).to_le_bytes());

    let result = TransitionTable::from_bytes(&data);
    assert!(matches!(result, Err(Error::InvalidTable { .. })));
}

#[test]
fn test_from_bytes_valid_minimal() {
    let mut data = vec![0; 8];
    data[0..4].copy_from_slice(&0u32.to_le_bytes());
    data[4..8].copy_from_slice(&0u32.to_le_bytes());
    
    // Accept count = 0
    data.extend_from_slice(&0u32.to_le_bytes());

    // Pat length count = 0
    data.extend_from_slice(&0u32.to_le_bytes());

    let result = TransitionTable::from_bytes(&data).unwrap();
    assert_eq!(result.state_count, 0);
    assert_eq!(result.class_count, 0);
}
