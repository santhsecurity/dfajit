use dfajit::JitDfa;
use matchkit::Match;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn test_concurrent_scan_and_compile_multithreaded() {
    let jit = Arc::new(JitDfa::from_patterns(&[b"apple", b"banana", b"cherry"]).unwrap());
    let barrier = Arc::new(Barrier::new(10));
    
    let mut handles = vec![];
    for _ in 0..10 {
        let jit = Arc::clone(&jit);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let mut matches = vec![Match::from_parts(0, 0, 0); 10];
            let input = b"an apple and a banana and a cherry in the apple tree";
            
            barrier.wait();
            
            for _ in 0..100 {
                let count = jit.scan(input, &mut matches);
                assert_eq!(count, 4);
                
                assert_eq!(matches[0].start(), 3);
                assert_eq!(matches[0].end(), 8);
                // "apple" pattern_id depends on insertion order. 
                // "apple" -> 0, "banana" -> 1, "cherry" -> 2
                assert_eq!(matches[0].pattern_id(), 0);
                
                assert_eq!(matches[1].start(), 15);
                assert_eq!(matches[1].end(), 21);
                assert_eq!(matches[1].pattern_id(), 1);
                
                assert_eq!(matches[2].start(), 28);
                assert_eq!(matches[2].end(), 34);
                assert_eq!(matches[2].pattern_id(), 2);
                
                assert_eq!(matches[3].start(), 42);
                assert_eq!(matches[3].end(), 47);
                assert_eq!(matches[3].pattern_id(), 0);
            }
        }));
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
}
