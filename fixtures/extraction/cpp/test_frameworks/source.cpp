class tst_QString : public QObject
{
    Q_OBJECT
private slots:
    void initTestCase();
    void append();
    void append_data();
    void chop();
    void cleanupTestCase();
public slots:
    void helper();
};

void tst_QString::append() { QCOMPARE(1, 1); }
void tst_QString::chop() {}

QTEST_MAIN(tst_QString)

TEST_SUITE("math") {
    TEST_CASE("mult") {
        SUBCASE("zero") { CHECK(true); }
    }
}

TEST_CASE_FIXTURE(Fixture, "fixture case") { CHECK(true); }

BOOST_AUTO_TEST_SUITE(math_suite)
BOOST_AUTO_TEST_CASE(test_add) { BOOST_CHECK_EQUAL(add(1, 2), 3); }
BOOST_FIXTURE_TEST_CASE(test_fix, Fixture) { BOOST_CHECK(true); }
BOOST_AUTO_TEST_SUITE_END()
