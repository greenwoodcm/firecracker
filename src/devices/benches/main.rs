// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::tempfile;
use tempfile::tempdir;
use std::io::{Result, Read, Write, Seek, SeekFrom};
use nix::sys::uio::pread;
use std::os::unix::io::AsRawFd;

#[inline]
pub fn bench_seek_read(file: &mut std::fs::File) {
    let mut buf = [0; 512];

    for i in 0..1000 {
        file.seek(SeekFrom::Start(0)).unwrap();
        file.read(&mut buf).unwrap();
    }
}

#[inline]
pub fn bench_pread(file: &mut std::fs::File) {
    let mut buf = [0; 512];

    for i in 0..1000 {
        pread(file.as_raw_fd(), &mut buf, 0 as i64);
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut temp_file = tempfile().unwrap();
    let buf = [0; 512];
    temp_file.write_all(&buf).unwrap();

    c.bench_function("Seek/read", |b| {
        b.iter(|| {
            bench_seek_read(
                black_box(&mut temp_file),
            )
        })
    });

    c.bench_function("Pread", |b| {
        b.iter(|| {
            bench_pread(
                black_box(&mut temp_file),
            )
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(200);
    targets = criterion_benchmark
}

criterion_main! {
    benches,
}
