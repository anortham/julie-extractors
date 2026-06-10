@registered
export class Worker {
    constructor(id) {
        this.id = id;
    }

    run() {
        return helper(this.id);
    }
}

/**
 * Increment a worker id.
 * @returns {number}
 */
function helper(value) {
    return value + 1;
}

function evaluate(count, enabled) {
    let total = 0;
    if (enabled) {
        for (let i = 0; i < count; i++) {
            total += i;
        }
    }
    return total;
}
