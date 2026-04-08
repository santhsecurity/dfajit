use dfajit::table::TransitionTable;
use dfajit::JitDfa;
use matchkit::Match;
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_dfa_scan_never_panics(input in any::<Vec<u8>>()) {
        let mut table = TransitionTable::new(2, 256).unwrap();
        table.set_transition(0, b'x', 1);
        table.add_accept(1, 0);
        table.set_pattern_length(0, 1);
        
        let dfa = JitDfa::compile(&table).unwrap();
        let mut matches = vec![Match::from_parts(0, 0, 0); 100];
        
        // This should not panic for any input
        let count = dfa.scan(&input, &mut matches);
        
        // Check bounds on matches
        assert!(count <= input.len());
        for i in 0..count {
            assert!(matches[i].start() < input.len());
            assert!(matches[i].end() <= input.len());
            assert!(matches[i].start() < matches[i].end());
            assert_eq!(matches[i].pattern_id(), 0);
        }
    }
}
