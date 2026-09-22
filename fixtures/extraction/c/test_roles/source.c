TestSuite(math, .init = setup_suite, .fini = teardown_suite);

Test(math, addition, .init = setup_test, .fini = teardown_test) {
    int sum = add(2, 2);
    cr_assert_eq(sum, 4);
}

void setup_suite(void) {}
void teardown_suite(void) {}
void setup_test(void) {}
void teardown_test(void) {}
void setup_unreferenced(void) {}
void helper_named_like_a_test(void) {}
void TestSuite_helper(void) {}
