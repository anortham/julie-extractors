package fixtures.extraction.java.test_roles;

import junit.framework.TestCase;

class LedgerSupport {
    static int total(int first, int second) {
        return first + second;
    }
}

abstract class AbstractLedgerTest {
    protected int running;

    protected void seed() {
        running = LedgerSupport.total(1, 2);
    }
}

class LedgerJUnit5Test extends AbstractLedgerTest {
    @org.junit.jupiter.api.BeforeAll
    static void beforeAll() {
        LedgerSupport.total(0, 0);
    }

    @org.junit.jupiter.api.AfterAll
    static void afterAll() {
        LedgerSupport.total(0, 0);
    }

    @org.junit.jupiter.api.BeforeEach
    void setUp() {
        seed();
    }

    @org.junit.jupiter.api.AfterEach
    void tearDown() {
        running = 0;
    }

    @org.junit.jupiter.api.Test
    void addsTwoNumbers() {
        LedgerSupport.total(running, 1);
    }

    @org.junit.jupiter.api.TestFactory
    void buildsDynamicCases() {
        LedgerSupport.total(2, 2);
    }

    @org.junit.jupiter.api.TestTemplate
    void runsTemplate() {
        LedgerSupport.total(3, 3);
    }

    @org.junit.jupiter.params.ParameterizedTest
    void addsEachPair() {
        LedgerSupport.total(4, 4);
    }

    @org.junit.jupiter.api.RepeatedTest
    void addsRepeatedly() {
        LedgerSupport.total(5, 5);
    }

    @org.junit.jupiter.api.Nested
    class WhenLedgerIsEmpty {
        @org.junit.jupiter.api.Test
        void reportsZero() {
            LedgerSupport.total(0, 0);
        }
    }
}

class LedgerJUnit4Test {
    @org.junit.Before
    public void before() {
        LedgerSupport.total(6, 6);
    }

    @org.junit.After
    public void after() {
        LedgerSupport.total(7, 7);
    }

    @org.junit.Test
    public void addsWithJUnit4() {
        LedgerSupport.total(8, 8);
    }
}

@org.testng.annotations.Test
class LedgerTestNgTest {
    @org.testng.annotations.BeforeSuite
    public void beforeSuite() {
        LedgerSupport.total(9, 9);
    }

    @org.testng.annotations.AfterSuite
    public void afterSuite() {
        LedgerSupport.total(9, 9);
    }

    @org.testng.annotations.BeforeTest
    public void beforeTest() {
        LedgerSupport.total(10, 10);
    }

    @org.testng.annotations.AfterTest
    public void afterTest() {
        LedgerSupport.total(10, 10);
    }

    @org.testng.annotations.BeforeGroups
    public void beforeGroups() {
        LedgerSupport.total(11, 11);
    }

    @org.testng.annotations.AfterGroups
    public void afterGroups() {
        LedgerSupport.total(11, 11);
    }

    @org.testng.annotations.BeforeMethod
    public void beforeMethod() {
        LedgerSupport.total(12, 12);
    }

    @org.testng.annotations.AfterMethod
    public void afterMethod() {
        LedgerSupport.total(12, 12);
    }

    public void addsWithTestNg() {
        LedgerSupport.total(13, 13);
    }

    private int helperTotal() {
        return LedgerSupport.total(14, 14);
    }
}

class LegacyLedgerTest extends TestCase {
    public void testAddsLegacy() {
        assertEquals(3, LedgerSupport.total(1, 2));
    }

    public void combineLegacy() {
        LedgerSupport.total(15, 15);
    }
}

class LedgerTestHelpers {
    public void testDataForLedger() {
        LedgerSupport.total(16, 16);
    }
}
