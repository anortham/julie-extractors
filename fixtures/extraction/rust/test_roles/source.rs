#[test]
fn extracts_test_case() {
    assert_eq!(2 + 2, 4);
}

#[tokio::test]
async fn extracts_async_test_case() {}

#[sqlx::test]
async fn extracts_qualified_suffix_test_case() {}

#[rstest]
fn extracts_plain_rstest_case() {}

#[rstest]
#[case(1)]
#[case(2)]
fn extracts_rstest_parameterized_case(#[case] input: u32) {
    let _ = input;
}

#[rstest]
#[case::six_times_seven(6, 7)]
fn extracts_rstest_named_case(#[case] left: u32, #[case] right: u32) {
    let _ = left * right;
}

#[test_case(1, 2)]
#[test_case(3, 4)]
fn extracts_test_case_macro_parameterized(left: u32, right: u32) {
    let _ = left + right;
}

#[fixture]
fn extracts_fixture_setup() -> u32 {
    7
}

fn test_named_but_unannotated_is_not_a_test() {}

#[tokio::main]
async fn qualified_non_test_attribute_is_not_a_test() {}

#[cfg(test)]
mod test_support {
    #[test]
    fn nested_case_inside_cfg_test_module() {}

    fn setup() {}
    fn teardown() {}
}

#[cfg(all(test, feature = "slow"))]
mod compound_cfg_support {
    #[test]
    fn nested_case_inside_compound_cfg_module() {}
}

mod ordinary_support {
    fn setup() {}
    fn teardown() {}
}

#[cfg(feature = "test")]
mod feature_support {}

#[cfg(not(test))]
mod negated_cfg_support {}
