use dfajit::{JitDfa, TransitionTable};
use matchkit::Match;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn test_concurrent_stress_compile_and_scan() {
    let mut table = TransitionTable::new(3, 256).unwrap();
    table.set_transition(0, b'x', 1);
    table.set_transition(1, b'y', 2);
    table.add_accept(2, 0);
    table.set_pattern_length(0, 2);

    let dfa = Arc::new(JitDfa::compile(&table).unwrap());
    let table_arc = Arc::new(table);

    // Stress test with 32 threads
    let num_threads = 32;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];

    for i in 0..num_threads {
        let barrier_clone = Arc::clone(&barrier);
        let dfa_clone = Arc::clone(&dfa);
        let table_clone = Arc::clone(&table_arc);

        handles.push(thread::spawn(move || {
            // Wait for all threads to be ready to maximize contention
            barrier_clone.wait();

            // Perform different operations based on thread ID
            if i % 3 == 0 {
                // Read-heavy path: Concurrent scanning
                let input = "xyxyxyxyxy".as_bytes();
                let mut matches = vec![Match::from_parts(0, 0, 0); 10];
                let count = dfa_clone.scan(input, &mut matches);
                assert_eq!(count, 5);
            } else if i % 3 == 1 {
                // Write-heavy path: Compiling new JitDfas concurrently
                // This puts stress on underlying OS virtual memory and allocator
                let local_dfa = JitDfa::compile(&*table_clone).unwrap();
                let count = local_dfa.scan_count(b"xy");
                assert_eq!(count, 1);
            } else {
                // State mutation path: Minimization concurrent with compilation
                let minimized = table_clone.minimize().unwrap_or((*table_clone).clone());
                let local_dfa = JitDfa::compile(&minimized).unwrap();
                let count = local_dfa.scan_count(b"xyxy");
                assert_eq!(count, 2);
            }
        }));
    }

    for handle in handles {
        let res = handle.join();
        assert!(res.is_ok(), "Thread panicked during concurrent stress test");
    }
}
