#include <utility>
struct Box { int value; };
int call(int value) { return value; }
int& ref_at(int value);
int writes(Box *box, int initial, int argument, int *values) {
    int initialized = initial;
    int plain = 0; plain = call(initial);
    int compound = 0; compound += 1; compound++;
    box->value = compound;
    values[initial] = compound;
    ref_at(initial) = 1;
    auto [captured, other] = std::pair<int, int>{initial, 0};
    call(captured);
    return call(argument);
}
