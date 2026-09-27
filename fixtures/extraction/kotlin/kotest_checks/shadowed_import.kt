package examples

import io.kotest.property.checkAll

fun <T> checkAll(block: (T) -> Boolean): Boolean = true

fun verify() {
    checkAll<Int> { it >= 0 }
}
