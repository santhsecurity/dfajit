# dfajit

JIT compilation of DFA transition tables to native x86_64 machine code.

## Quick Start

```rust
use dfajit::{JitDfa, TransitionTable};

let mut table = TransitionTable::new(3, 256);
table.set_transition(0, b'a', 1);
table.set_transition(1, b'b', 2);
table.add_accept(2, 0);
table.set_pattern_length(0, 2);

let jit = JitDfa::compile(&table).unwrap();
let mut matches = Vec::new();
assert_eq!(jit.scan(b"xabxab", &mut matches), 2);
```

## Features

- Real x86_64 machine code emission (not interpreted)
- W^X memory safety (RW → RX via mprotect)
- Hopcroft DFA minimization
- Range analysis for multi-byte stride optimization
- Serialization (to_bytes / from_bytes)
- Convenience builders: `from_patterns`, `from_regex_patterns`
- scan_count, scan_first, has_match fast paths
- Interpreted fallback for large DFAs (>4096 states)

## License

MIT
