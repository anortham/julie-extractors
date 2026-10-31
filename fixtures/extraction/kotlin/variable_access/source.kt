class Box { var value: Int = 0 }
fun call(value: Int) = value
fun run(box: Box, seed: Int, pair: Pair<Int, Int>, values: IntArray) {
    var initialized = seed
    var plain = 0; plain = call(seed)
    var compound = 0; compound += 1; compound++
    box.value = compound
    values[seed] = compound
    val (captured, other) = pair
    call(captured)
}
