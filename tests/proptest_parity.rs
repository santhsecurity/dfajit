//! Property-based tests verifying DFA scan invariants.
#![allow(clippy::cast_possible_truncation, clippy::unwrap_used)]

use dfajit::{JitDfa, TransitionTable};
use proptest::prelude::*;

fn arb_dfa() -> impl Strategy<Value = TransitionTable> {
    (
        2usize..=32,
        prop::collection::vec(0u32..32, 2..=32),
        prop::collection::vec(0u32..32, 2 * 256..=32 * 256),
    )
        .prop_map(|(state_count, accept_seed, transition_seed)| {
            let mut table = TransitionTable::new(state_count, 256).unwrap();
            let accept_total = accept_seed.len().clamp(1, state_count);

            let mut index = 0usize;
            for state in 0..state_count {
                for byte in u8::MIN..=u8::MAX {
                    let seed = transition_seed[index % transition_seed.len()];
                    table.set_transition(state, byte, seed % state_count as u32);
                    index += 1;
                }
            }

            let mut used_states = vec![false; state_count];
            for (pattern_id, accept_state) in
                accept_seed.iter().copied().enumerate().take(accept_total)
            {
                let accept_state = accept_state % state_count as u32;
                if !used_states[accept_state as usize] {
                    used_states[accept_state as usize] = true;
                    table.add_accept(accept_state, pattern_id as u32);
                    table.set_pattern_length(pattern_id as u32, 1);
                }
            }

            table
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn scan_count_matches_materialized_matches(
        table in arb_dfa(),
        input in prop::collection::vec(any::<u8>(), 0..2048),
    ) {
        let jit = JitDfa::compile(&table).unwrap();
        let counted = jit.scan_count(&input);

        let mut matches = vec![matchkit::Match::from_parts(0, 0, 0); input.len()];
        let scanned = jit.scan(&input, &mut matches);
        matches.truncate(scanned);

        prop_assert_eq!(counted, matches.len());
        prop_assert_eq!(scanned, matches.len());
        prop_assert!(matches.len() <= input.len());

        for found in &matches {
            prop_assert!(found.start <= found.end);
            prop_assert!((found.end as usize) <= input.len());
            prop_assert!((found.start as usize) <= input.len());
            prop_assert!((found.pattern_id as usize) < table.pattern_lengths().len());
        }
    }

    #[test]
    fn empty_input_never_matches(table in arb_dfa()) {
        let jit = JitDfa::compile(&table).unwrap();
        prop_assert_eq!(jit.scan_count(b""), 0);
    }

    #[test]
    fn serialization_round_trip_preserves_table(table in arb_dfa()) {
        let bytes = table.to_bytes();
        let restored = TransitionTable::from_bytes(&bytes).unwrap();

        prop_assert_eq!(restored.state_count(), table.state_count());
        prop_assert_eq!(restored.class_count(), table.class_count());
        prop_assert_eq!(restored.transitions(), table.transitions());
        prop_assert_eq!(restored.accept_states(), table.accept_states());
        prop_assert_eq!(restored.pattern_lengths(), table.pattern_lengths());
    }
}
