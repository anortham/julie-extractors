#[test]
fn extracts_test_case() {
    assert_eq!(2 + 2, 4);
}

fn test_named_but_unannotated_is_not_a_test() {}

#[cfg(test)]
mod test_support {
    fn setup() {}
    fn teardown() {}
}

mod ordinary_support {
    fn setup() {}
    fn teardown() {}
}

#[cfg(feature = "test")]
mod feature_support {}
