package main
type Holder struct { value int }
func consume(int) {}
func writes(initial int, holder Holder, argument int, records []int) {
 initialized := initial
 plain := 0; plain = initial
 compound := 0; compound += initial
 holder.value = initial
 items := []int{0}; offset := 0; index := 0; items[offset + index] = initial
 counter := 0; counter++
 consume(argument)
    for captured := range records { consume(captured) }
}
