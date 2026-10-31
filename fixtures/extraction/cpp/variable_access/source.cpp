#include <utility>
constexpr int kSize = 3;
constexpr int size_for(int value) { return value; }
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
    int arr[kSize] = {initial};
    int matrix[size_for(kSize)][kSize] = {};
    int direct = initial; direct = call(initial);
    return call(argument);
}
