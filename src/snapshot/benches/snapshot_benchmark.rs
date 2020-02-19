extern crate criterion;
extern crate snapshot;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use snapshot::{bench_1mil_save_restore, Snapshot};
use std::path::Path;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("bench 1", |b| b.iter(|| bench_1mil_save_restore()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
