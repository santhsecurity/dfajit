use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dfajit::{JitDfa, TransitionTable};

fn build_linear_dfa(state_count: usize) -> TransitionTable {
    let mut table = TransitionTable::new(state_count, 256).unwrap();
    for state in 0..state_count {
        for byte in u8::MIN..=u8::MAX {
            table.set_transition(state, byte, 0);
        }
    }

    for state in 0..state_count.saturating_sub(1) {
        let needle = b'a' + u8::try_from(state % 26).unwrap_or(0);
        let next_state = u32::try_from(state + 1).unwrap_or(u32::MAX);
        table.set_transition(state, needle, next_state);
    }

    let accept_state = u32::try_from(state_count.saturating_sub(1)).unwrap_or(u32::MAX);
    table.add_accept(accept_state, 0);
    let pattern_length = u32::try_from(state_count.saturating_sub(1)).unwrap_or(u32::MAX);
    table.set_pattern_length(0, pattern_length);
    table
}

fn no_match_input() -> Vec<u8> {
    vec![b'x'; 1024 * 1024]
}

fn one_percent_match_input() -> Vec<u8> {
    let mut input = vec![b'x'; 1024 * 1024];
    let pattern = b"abc";
    for index in (0..input.len().saturating_sub(pattern.len())).step_by(100) {
        input[index..index + pattern.len()].copy_from_slice(pattern);
    }
    input
}

fn bench_compile_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile_latency");
    for &state_count in &[3usize, 11, 100] {
        let table = build_linear_dfa(state_count);
        group.bench_with_input(
            BenchmarkId::from_parameter(state_count),
            &table,
            |b, table| {
                b.iter(|| {
                    if let Ok(jit) = JitDfa::compile(black_box(table)) {
                        black_box(jit);
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_scan_throughput(c: &mut Criterion) {
    let table = build_linear_dfa(4);
    let Ok(jit) = JitDfa::compile(&table) else {
        return;
    };
    let mut group = c.benchmark_group("scan_throughput");

    let no_match = no_match_input();
    group.throughput(Throughput::Bytes(no_match.len() as u64));
    group.bench_function("no_match_1mb", |b| {
        b.iter(|| {
            let mut matches = Vec::new();
            black_box(jit.scan(black_box(&no_match), &mut matches));
            black_box(matches);
        });
    });

    let one_percent = one_percent_match_input();
    group.throughput(Throughput::Bytes(one_percent.len() as u64));
    group.bench_function("one_percent_match_1mb", |b| {
        b.iter(|| {
            let mut matches = Vec::new();
            black_box(jit.scan(black_box(&one_percent), &mut matches));
            black_box(matches);
        });
    });

    group.finish();
}

criterion_group!(jit_benches, bench_compile_latency, bench_scan_throughput);
criterion_main!(jit_benches);
