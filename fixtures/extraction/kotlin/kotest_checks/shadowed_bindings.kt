package examples

import io.kotest.property.checkAll
import io.kotest.property.forAll

fun parameterScope(checkAll: (() -> Unit) -> Unit) {
    checkAll { }
}

fun localScope() {
    checkAll<Int> { it >= 0 }
    val checkAll: (() -> Unit) -> Unit = {}
    checkAll { }
}

fun nestedScope() {
    run {
        val forAll: (() -> Unit) -> Unit = {}
        forAll { }
    }
    forAll<Int> { it >= 0 }
}
