type Props = {
    label: string;
};

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

function format(value: string): string {
    return value.trim();
}
