package examples

import io.kotest.data.forAll as forAllRows
import io.kotest.data.forNone
import io.kotest.data.row
import io.kotest.datatest.withData
import io.kotest.property.checkAll
import io.kotest.property.forAll
import org.junit.jupiter.api.Test

class KotestChecks {
    @Test
    fun checks() {
        forAllRows(row(2, 4), row(3, 9)) { base, square -> base * base == square }
        forNone(row(5, 25)) { value, square -> value + value != square }
        checkAll<Int> { value -> value == value }
        checkAll<Int, String> { value, label -> value >= 0 && label.isNotEmpty() }
        forAll<String> { value -> value.length >= 0 }
        listOf(1, 2, 3).forAll { it > 0 }
        withData(1, 2) { value -> value >= 1 }
    }

    private fun List<Int>.forAll(predicate: (Int) -> Boolean): Boolean = all(predicate)
}
