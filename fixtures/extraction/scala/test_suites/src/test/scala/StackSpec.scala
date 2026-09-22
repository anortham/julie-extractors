class StackWordSpec extends AnyWordSpec {
  "A Stack" when {
    "empty" should {
      "have size 0" in { assert(true) }
    }
  }
}

class StackFreeSpec extends AnyFreeSpec {
  "A Cart" - {
    "adds items" in { assert(true) }
  }
}

class StackFeatureSpec extends AnyFeatureSpec {
  Feature("Stack") {
    Scenario("push then pop") { assert(true) }
  }
}

class CartMunit extends munit.FunSuite {
  test("skipped".ignore) { assert(true) }
  test("slow".tag(Slow)) { assert(true) }
  tmp.test("with fixture") { s => assertEquals(s, "x") }
  override def beforeAll(): Unit = ()
}

class CartFunSpec extends AnyFunSpec {
  describe("Cart") {
    ignore("not yet") { fail() }
  }
}

class PlainHelper {
  def testData(): Int = 1
}
