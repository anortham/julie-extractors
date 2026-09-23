import io.kotest.core.spec.style.BehaviorSpec

class CartSpec : BehaviorSpec({
    beforeSpec {
        reset()
    }

    Given("an empty cart") {
        `when`("an item is added") {
            Then("the total grows") {
                check()
            }
        }
        xGiven("a disabled branch") {
            then("never runs") {
                check()
            }
        }
    }

    context("pythag triples") {
        withData(Triple(3, 4, 5), Triple(6, 8, 10)) { (a, b, c) ->
            check()
        }
    }
})
