void setUp(void) { init_math(); }
void tearDown(void) { cleanup_math(); }
void suiteSetUp(void) { }
int suiteTearDown(int failures) { return failures; }
void test_add_works(void) { TEST_ASSERT_EQUAL(4, add(2, 2)); }

static int group_setup(void **state) { return 0; }
static int group_teardown(void **state) { return 0; }
static int each_setup(void **state) { return 0; }
static void list_push_increments_length(void **state) { }
static void list_pop(void **state) { }

int main(void) {
    const struct CMUnitTest tests[] = {
        cmocka_unit_test(list_push_increments_length),
        cmocka_unit_test_setup_teardown(list_pop, each_setup, NULL),
    };
    return cmocka_run_group_tests(tests, group_setup, group_teardown);
}
