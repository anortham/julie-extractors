enum { SIZE = 3 };
struct Box { int value; };
int call(int value) { return value; }
int size_for(int value) { return value; }
int writes(struct Box *box, int initial, int argument, int *values) {
    int initialized = initial;
    int plain = 0; plain = call(initial);
    int compound = 0; compound += 1; compound++;
    box->value = compound;
    values[initial] = compound;
    int buf[SIZE] = {initial};
    int matrix[size_for(initial)][SIZE];
    int assigned[(initial = 4)];
    int incremented[++initial];
    int direct = initial; direct = initial;
    return call(argument);
}
