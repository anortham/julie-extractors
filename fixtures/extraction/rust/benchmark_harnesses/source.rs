use criterion::{criterion_group, criterion_main, Criterion};

#[bench]
fn libtest_benchmark(_: &mut test::Bencher) {}

#[divan::bench(args = [1, 2], sample_count = 100)]
fn divan_benchmark(_: u64) {}

fn criterion_fast(_: &mut Criterion) {}
fn criterion_slow(_: &mut Criterion) {}

criterion_group!(criterion_benches, criterion_fast, criterion_slow);
criterion_group! {
    name = criterion_complete;
    config = Criterion::default();
    targets = criterion_slow, criterion_fast
}
criterion_main!(criterion_benches, criterion_complete);

#[test]
fn ordinary_test() {}

#[custom::bench]
fn unrelated_attribute() {}

fn ordinary_function() {}

custom_group!(ordinary_function);
