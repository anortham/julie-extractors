const counter = 1;
const state = { value: 2 };

export function readCounter() {
    return counter;
}

export function readValue() {
    return state.value;
}
