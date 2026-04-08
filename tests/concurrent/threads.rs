#![allow(clippy::unwrap_used, clippy::panic)]

use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn test_concurrent_scan_and_compile() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'a', 1);
    table.set_transition(1, b'b', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let dfa = Arc::new(JitDfa::compile(&table).unwrap());
    let table_arc = Arc::new(table);

    let num_threads = 50;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];

    for i in 0..num_threads {
        let barrier_clone = Arc::clone(&barrier);
        let dfa_clone = Arc::clone(&dfa);
        let table_clone = Arc::clone(&table_arc);

        handles.push(thread::spawn(move || {
            // Wait for all threads to be ready to maximize contention
            barrier_clone.wait();

            if i % 2 == 0 {
                // Half the threads scan concurrently
                let mut matches = vec![Match::from_parts(0, 0, 0); 100];
                let count = dfa_clone.scan(b"abababxabab", &mut matches);
                assert_eq!(count, 5);
            } else {
                // Half compile a new instance and minimize concurrently
                let minimized = table_clone.minimize().unwrap_or((*table_clone).clone());
                let local_dfa = JitDfa::compile(&minimized).unwrap();
                let mut matches = vec![Match::from_parts(0, 0, 0); 10];
                let count = local_dfa.scan(b"xabx", &mut matches);
                assert_eq!(count, 1);
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
