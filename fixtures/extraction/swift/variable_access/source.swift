struct Holder { var value: Int }
func consume(_ value: Int) {}
func bump(_ value: inout Int) {}
func writes(initial: Int, box: Holder, argument: Int, records: [Int]) {
 let initialized = initial
 var plain = 0; plain = initial
 var compound = 0; compound += initial
 box.value = initial
 var items = [0]; let offset = 0; let index = 0; items[offset + index] = initial
 consume(argument); bump(&argument)
 for captured in records { consume(captured) }
}
