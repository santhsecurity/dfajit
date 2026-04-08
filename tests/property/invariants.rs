#![allow(clippy::unwrap_used, clippy::panic)]

use dfajit::{JitDfa, TransitionTable};
use proptest::prelude::*;

fn arb_dfa() -> impl Strategy<Value = TransitionTable> {
    (
        2usize..=10,
        prop::collection::vec(0u32..10, 2..=10),
        prop::collection::vec(0u32..10, 2 * 256..=10 * 256),
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

            // Compilation requires that an accept state has exactly 1 pattern ID
            // Hopcroft minimization considers two accept states equivalent ONLY if they
            // have the same pattern ID. Since this is an abstract test, we'll force
            // all accept states to map to pattern 0 so minimization doesn't fail compilation
            // due to state overlap logic issues in the random test generator.
            let mut used_states = vec![false; state_count];
            for accept_state in accept_seed.iter().copied().take(accept_total) {
                let accept_state = accept_state % state_count as u32;
                if !used_states[accept_state as usize] {
                    used_states[accept_state as usize] = true;
                    table.add_accept(accept_state, 0);
                    table.set_pattern_length(0, 1);
                }
            }

            table
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn serialization_roundtrip_preserves_everything(table in arb_dfa()) {
        let bytes = table.to_bytes();
        let restored = TransitionTable::from_bytes(&bytes).unwrap();

        prop_assert_eq!(restored.state_count(), table.state_count());
        prop_assert_eq!(restored.class_count(), table.class_count());
        prop_assert_eq!(&restored.transitions(), &table.transitions());
        prop_assert_eq!(&restored.accept_states(), &table.accept_states());
        prop_assert_eq!(&restored.pattern_lengths(), &table.pattern_lengths());

        let jit_a = JitDfa::compile(&table).unwrap();
        let jit_b = JitDfa::compile(&restored).unwrap();

        prop_assert_eq!(jit_a.state_count(), jit_b.state_count());
        prop_assert_eq!(jit_a.pattern_count(), jit_b.pattern_count());
    }

    #[test]
    fn minimization_preserves_matches(
        table in arb_dfa(),
        input in prop::collection::vec(any::<u8>(), 0..512)
    ) {
        let original_jit = JitDfa::compile(&table).unwrap();
        let orig_count = original_jit.scan_count(&input);

        if let Some(minimized) = table.minimize() {
            let min_jit = JitDfa::compile(&minimized).unwrap();
            let min_count = min_jit.scan_count(&input);
            prop_assert_eq!(orig_count, min_count);
        }
    }
}
