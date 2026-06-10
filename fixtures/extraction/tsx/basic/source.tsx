type Props = {
    label: string;
};

@Component()
export class WorkerModel {
    constructor(public label: string) {}
}

function fetchWorkers() {
    return fetch("/api/workers");
}

function observeRun(event: string) {
    void event;
}

export function Badge(props: Props) {
    return (
        <button data-action="run" onClick={() => observeRun("worker-run")}>
            {format(props.label)}
        </button>
    );
}

/**
 * Format the badge label.
 * @returns formatted string
 */
function format(value: string): string {
    return value.trim();
}

function evaluate(count: number, enabled: boolean): number {
    let total = 0;
    if (enabled) {
        for (let i = 1; i <= count; i++) {
            total += i;
        }
    } else if (count > 0) {
        total = 1;
    }
    return total;
}
