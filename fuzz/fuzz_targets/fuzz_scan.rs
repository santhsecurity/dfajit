#![no_main]
use libfuzzer_sys::fuzz_target;
use dfajit::{TransitionTable, JitDfa};

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    // Build a simple 3-state DFA matching "ab"
    let mut table = TransitionTable::new(3, 256);
    for state in 0..3 {
        for byte in 0..=255u8 {
            table.set_transition(state, byte, 0);
        }
        table.set_transition(state, b'a', 1);
    }
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let Ok(jit) = JitDfa::compile(&table) else {
        return;
    };

    // Scan with arbitrary input — must not panic or crash
    let mut matches = Vec::new();
    let _ = jit.scan(data, &mut matches);
});
