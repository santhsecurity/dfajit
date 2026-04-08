use dfajit::TransitionTable;
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_serialization_round_trip(
        state_count in 1..100usize,
        class_count in 1..256usize,
        transitions in prop::collection::vec(0..1000u32, 0..25600),
        accept_states in prop::collection::vec((0..100u32, 0..100u32), 0..100),
        pattern_lengths in prop::collection::vec(0..1000u32, 0..100)
    ) {
        let mut table = TransitionTable::new(state_count, class_count).unwrap();
        
        let trans_len = state_count.saturating_mul(class_count);
        let mut actual_transitions = transitions;
        actual_transitions.resize(trans_len, 0);
        
        *table.transitions_mut() = actual_transitions;
        *table.accept_states_mut() = accept_states;
        *table.pattern_lengths_mut() = pattern_lengths;

        let bytes = table.to_bytes();
        let restored = TransitionTable::from_bytes(&bytes).unwrap();

        assert_eq!(table.state_count(), restored.state_count());
        assert_eq!(table.class_count(), restored.class_count());
        assert_eq!(table.transitions(), restored.transitions());
        assert_eq!(table.accept_states(), restored.accept_states());
        assert_eq!(table.pattern_lengths(), restored.pattern_lengths());
    }
}
