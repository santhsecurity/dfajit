import sys

out = """//! Exhaustive adversarial tests for dfajit JIT DFA compiler.
#![allow(clippy::unwrap_used, clippy::panic, unused_imports)]

use dfajit::{JitDfa, TransitionTable, Error};
use matchkit::Match;
use std::sync::Arc;

fn reset_table(state_count: usize) -> TransitionTable {
    let mut table = TransitionTable::new(state_count, 256).unwrap();
    for state in 0..state_count {
        for byte in u8::MIN..=u8::MAX {
            table.set_transition(state, byte, 0);
        }
    }
    table
}

"""

# 1-5: State explosion
for i in range(1, 6):
    out += f"""#[test]
fn test_state_explosion_{i}() {{
    // Simulate state explosion
    let num_states = 4096;
    let mut table = reset_table(num_states);
    for s in 0..num_states - 1 {{
        table.set_transition(s, b'a', (s + 1) as u32);
        table.set_transition(s, b'b', (s + 1) as u32);
    }}
    table.add_accept((num_states - 1) as u32, 0);
    table.set_pattern_length(0, {i});
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    let input = vec![b'a'; num_states];
    jit.scan(&input, &mut matches);
}}
"""

# 6-10: Max state count near limits
out += """#[test]
fn test_max_states_4096() {
    let table = reset_table(4096);
    assert!(JitDfa::compile(&table).is_ok());
}

#[test]
fn test_max_states_4097_fails() {
    let table = reset_table(4097);
    assert!(JitDfa::compile(&table).is_err());
}

#[test]
fn test_max_states_dense() {
    let mut table = reset_table(4096);
    for s in 0..4095 {
        for b in 0..=255 {
            table.set_transition(s, b, (s + 1) as u32);
        }
    }
    table.add_accept(4095, 0);
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![0; 4095];
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(&input, &mut matches), 1);
}

#[test]
fn test_max_states_ping_pong() {
    let mut table = reset_table(4096);
    table.set_transition(0, b'x', 4095);
    table.set_transition(4095, b'x', 0);
    table.add_accept(4095, 0);
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let input = b"xxxx";
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(input, &mut matches), 2);
}

#[test]
fn test_max_states_all_accepts() {
    let mut table = reset_table(4096);
    for s in 1..4096 {
        table.set_transition(s - 1, b'a', s as u32);
        table.add_accept(s as u32, 0);
    }
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![b'a'; 4095];
    let mut matches = vec![Match::from_parts(0, 0, 0); 5000];
    assert_eq!(jit.scan(&input, &mut matches), 4095);
}
"""

# 11-15: Boundaries (1, 255, 4096, 65536)
for i, size in enumerate([1, 255, 4096, 65536, 0]):
    out += f"""#[test]
fn test_boundary_input_{size}() {{
    let mut table = reset_table(2);
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![b'x'; {size}];
    let mut matches = vec![Match::from_parts(0, 0, 0); {max(10, size + 1)}];
    assert_eq!(jit.scan(&input, &mut matches), {size});
}}
"""

# 16-20: Binary input
binaries = [
    ("all_nulls", "vec![0x00; 1000]", "0x00"),
    ("all_ffs", "vec![0xFF; 1000]", "0xFF"),
    ("alternating", "(0..1000).map(|i| if i % 2 == 0 { 0x00 } else { 0xFF }).collect::<Vec<u8>>()", "0xFF"),
    ("random", "(0..1000).map(|i| (i * 17 % 256) as u8).collect::<Vec<u8>>()", "0x11"),
    ("high_bit", "(0..1000).map(|i| (i % 128 + 128) as u8).collect::<Vec<u8>>()", "0x80"),
]

for i, (name, input_gen, trigger) in enumerate(binaries):
    out += f"""#[test]
fn test_binary_input_{name}() {{
    let mut table = reset_table(2);
    table.set_transition(0, {trigger}, 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let input = {input_gen};
    let mut matches = vec![Match::from_parts(0, 0, 0); 2000];
    jit.scan(&input, &mut matches); // Just ensure it doesn't crash
}}
"""

# 21-25: Overlapping pattern matches
out += """#[test]
fn test_overlap_same_offset_1() {
    let mut table = reset_table(3);
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'a', 2);
    table.add_accept(1, 0);
    table.add_accept(2, 1);
    table.set_pattern_length(0, 1);
    table.set_pattern_length(1, 2);
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"aa", &mut matches), 2);
}

#[test]
fn test_overlap_same_offset_10_patterns() {
    let mut table = reset_table(2);
    table.set_transition(0, b'x', 1);
    for p in 0..10 {
        table.add_accept(1, p);
        table.set_pattern_length(p, 1);
    }
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 20];
    assert_eq!(jit.scan(b"x", &mut matches), 10);
}

#[test]
fn test_overlap_same_offset_100_patterns() {
    let mut table = reset_table(2);
    table.set_transition(0, b'y', 1);
    for p in 0..100 {
        table.add_accept(1, p);
        table.set_pattern_length(p, 1);
    }
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 200];
    assert_eq!(jit.scan(b"y", &mut matches), 100);
}

#[test]
fn test_overlap_different_lengths_same_byte() {
    let mut table = reset_table(3);
    table.set_transition(0, b'z', 1);
    table.set_transition(1, b'z', 2);
    table.add_accept(2, 0);
    table.add_accept(2, 1);
    table.set_pattern_length(0, 1);
    table.set_pattern_length(1, 2);
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"zz", &mut matches), 2);
}

#[test]
fn test_overlap_all_256_bytes() {
    let mut table = reset_table(2);
    for b in 0..=255 {
        table.set_transition(0, b, 1);
    }
    table.add_accept(1, 0);
    table.add_accept(1, 1);
    table.set_pattern_length(0, 1);
    table.set_pattern_length(1, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![0x42];
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(&input, &mut matches), 2);
}
"""

# 26-30: Zero-length, longer than input
out += """#[test]
fn test_pattern_longer_than_input() {
    let mut table = reset_table(4);
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.set_transition(2, b'c', 3);
    table.add_accept(3, 0);
    table.set_pattern_length(0, 3);
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"ab", &mut matches), 0);
}

#[test]
fn test_zero_length_match() {
    let mut table = reset_table(2);
    table.set_transition(0, b'x', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 0); // Zero length
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"x", &mut matches), 1);
    assert_eq!(matches[0].start, matches[0].end);
}

#[test]
fn test_zero_length_every_byte() {
    let mut table = reset_table(2);
    for b in 0..=255 {
        table.set_transition(0, b, 1);
    }
    table.add_accept(1, 0);
    table.set_pattern_length(0, 0);
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"abc", &mut matches), 3);
}

#[test]
fn test_input_len_1_pattern_len_2() {
    let mut table = reset_table(3);
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"a", &mut matches), 0);
}

#[test]
fn test_input_len_0_pattern_len_1() {
    let mut table = reset_table(2);
    table.set_transition(0, b'a', 1);
    table.add_accept(1, 0);
    table.set_pattern_length(0, 1);
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 10];
    assert_eq!(jit.scan(b"", &mut matches), 0);
}
"""

# 31-33: Max patterns (1000)
out += """#[test]
fn test_1000_patterns_diff_bytes() {
    let mut table = reset_table(1001);
    for i in 0..1000 {
        let b = (i % 256) as u8;
        table.set_transition(0, b, (i + 1) as u32);
        table.add_accept((i + 1) as u32, i as u32);
        table.set_pattern_length(i as u32, 1);
    }
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 2000];
    let input = vec![0, 1, 2];
    jit.scan(&input, &mut matches); // Check no panic
}

#[test]
fn test_1000_patterns_same_byte() {
    let mut table = reset_table(2);
    table.set_transition(0, b'x', 1);
    for i in 0..1000 {
        table.add_accept(1, i as u32);
        table.set_pattern_length(i as u32, 1);
    }
    let jit = JitDfa::compile(&table).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 2000];
    assert_eq!(jit.scan(b"x", &mut matches), 1000);
}

#[test]
fn test_1000_patterns_varying_lengths() {
    let mut table = reset_table(1001);
    for i in 0..1000 {
        table.set_transition(i as u32, b'a', (i + 1) as u32);
        table.add_accept((i + 1) as u32, i as u32);
        table.set_pattern_length(i as u32, (i + 1) as u32);
    }
    let jit = JitDfa::compile(&table).unwrap();
    let input = vec![b'a'; 1000];
    let mut matches = vec![Match::from_parts(0, 0, 0); 2000];
    assert_eq!(jit.scan(&input, &mut matches), 1000);
}
"""

with open("libs/performance/matching/dfajit/tests/adversarial_jit.rs", "w") as f:
    f.write(out)

print("Generated exactly 33 tests.")
