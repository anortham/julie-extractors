struct Box { int value; };
int call(int value) { return value; }
int writes(struct Box *box, int initial, int argument, int *values) {
    int initialized = initial;
    int plain = 0; plain = call(initial);
    int compound = 0; compound += 1; compound++;
    box->value = compound;
    values[initial] = compound;
    return call(argument);
}
