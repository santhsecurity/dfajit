use dfajit::JitDfa;
use matchkit::Match;

fn main() {
    let jit = JitDfa::from_regex_patterns(&["a+"]).unwrap();
    let mut matches = vec![Match::from_parts(0, 0, 0); 20];
    let count = jit.scan(b"aaab", &mut matches);
    println!("count = {}", count);
    for i in 0..count.min(matches.len()) {
        let m = matches[i];
        println!(
            "match {}: pid={} start={} end={}",
            i, m.pattern_id, m.start, m.end
        );
    }
}
