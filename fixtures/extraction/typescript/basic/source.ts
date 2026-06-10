export interface Job {
    run(): number;
}

@Component()
export class Worker implements Job {
    constructor(private id: number) {}

    run(): number {
        return helper(this.id);
    }
}

/**
 * Increment a worker id.
 * @param value the worker id
 * @returns the incremented id
 */
function helper(value: number): number {
    return value + 1;
}

function evaluate(count: number, enabled: boolean): number {
    let total = 0;
    if (enabled) {
        for (let i = 0; i < count; i++) {
            total += i;
        }
    }
    return total;
}
