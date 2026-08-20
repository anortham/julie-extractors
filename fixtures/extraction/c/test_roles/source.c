TestSuite(math, .init = setup_suite, .fini = teardown_suite);

Test(math, addition, .init = setup_test, .fini = teardown_test) {
    cr_assert_eq(2 + 2, 4);
}

void setup_suite(void) {}
void teardown_suite(void) {}
void setup_test(void) {}
void teardown_test(void) {}
void setup_unreferenced(void) {}
void helper_named_like_a_test(void) {}
void TestSuite_helper(void) {}
