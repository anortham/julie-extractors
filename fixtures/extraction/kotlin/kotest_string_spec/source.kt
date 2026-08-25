import io.kotest.core.spec.style.FreeSpec
import io.kotest.core.spec.style.StringSpec
import io.kotest.core.spec.style.WordSpec
import io.kotest.core.spec.style.funSpec
import io.kotest.matchers.shouldBe

private val sharedCases = funSpec {
    beforeEach {
        seedFixtures()
    }

    test("shares a case with every including spec") {
        1 shouldBe 1
    }
}

class LengthStringSpec : StringSpec({
    beforeTest {
        seedFixtures()
    }

    "length returns the size of the string" {
        "hello".length shouldBe 5
    }

    "startsWith matches a prefix" {
        "world".startsWith("wor") shouldBe true
    }

    afterTest {
        clearFixtures()
    }
})

class LengthWordSpec : WordSpec({
    "String.length" should {
        "return the length of the string" {
            "sam".length shouldBe 3
        }

        "return zero for an empty string" {
            "".length shouldBe 0
        }
    }
})

class LengthFreeSpec : FreeSpec({
    "String.length" - {
        "returns the length of the string" {
            "sam".length shouldBe 3
        }
    }
})

class LengthHelpers {
    fun seedRows(): Int {
        return 3
    }
}
