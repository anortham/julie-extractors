package examples

import io.kotest.data.*

fun verify() {
    forAll(row(1, 1)) { input, expected -> input == expected }
    io.kotest.property.checkAll<Int> { value -> value >= 0 }
}
