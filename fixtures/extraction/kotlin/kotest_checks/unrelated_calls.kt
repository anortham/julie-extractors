package examples

fun row(value: Int): Int = value

fun forAll(value: Int, block: (Int) -> Boolean): Boolean = block(value)

fun <T> checkAll(block: (T) -> Boolean): Boolean = true

fun unrelated() {
    forAll(row(1)) { it > 0 }
    checkAll<Int> { it > 0 }
}
