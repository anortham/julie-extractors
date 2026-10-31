class Box { int value; }
class Sample {
    int call(int value) { return value; }
    void run(Box box, int seed, int[] values) {
        int initialized = seed;
        int plain = 0; plain = call(seed);
        int compound = 0; compound += 1; compound++;
        box.value = compound;
        values[seed] = compound;
        if (seed instanceof Integer captured) { call(captured); }
    }
}
