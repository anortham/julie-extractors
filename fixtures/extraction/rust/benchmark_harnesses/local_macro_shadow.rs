use criterion::{criterion_group, criterion_main};

fn benchmark_target() {}

criterion_group!(local_benchmark_group, benchmark_target);
criterion_main!(local_benchmark_group);

macro_rules! criterion_main {
    ($group:ident) => {};
}

criterion_main!(shadowed_group);
