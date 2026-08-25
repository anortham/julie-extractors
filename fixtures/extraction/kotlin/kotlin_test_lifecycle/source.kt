import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals

class LedgerKotlinTest {
    @BeforeTest
    fun prepareLedger() {
    }

    @AfterTest
    fun clearLedger() {
    }

    @Test
    fun postsAnEntry() {
        assertEquals(1, 1)
    }
}

@Test
class LedgerTestNgTest {
    fun postsAnEntry() {
    }

    private fun helperTotal(): Int {
        return 0
    }

    @BeforeMethod
    fun prepareRun() {
    }
}

class LedgerTestHelpers {
    fun testDataForLedger(): String {
        return "rows"
    }
}
