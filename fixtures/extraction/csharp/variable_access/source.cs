class Box { public int Value; }
class Sample {
    int Call(int value) => value;
    void Ref(ref int value) { }
    void Out(out int value) { value = 0; }
    int GetIndex(int value) => value;
    int GetIndexRef(ref int value) => value;
    unsafe void Store(int* ptr) {
        *ptr = 1;
        *ptr += 1;
    }
    void Run(Box box, int seed, int[] values) {
        int initialized = seed;
        int plain = 0; plain = Call(seed);
        int compound = 0; compound += 1; compound++;
        box.Value = compound;
        Ref(ref box.Value);
        Out(out compound);
        values[GetIndex(seed)] = compound;
        values[GetIndexRef(ref seed)] = compound;
        values[seed] = compound;
        if (seed is int captured) { Call(captured); }
    }
}
