fn benchmark_target() {}

criterion::criterion_group!(qualified_group, benchmark_target);
criterion::criterion_main!(qualified_group);
criterion_group!(custom_group, benchmark_target);
criterion_main!(custom_group);
custom_registration!(benchmark_target);
