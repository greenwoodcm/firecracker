extern crate criterion;
extern crate snapshot;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use snapshot::{bench_restore_v1, Snapshot};
use std::path::Path;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("bench 1", |b| b.iter(|| bench_restore_v1()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
