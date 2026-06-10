export function Badge({ label }) {
    function handleClick() {
        fetch("/api/workers");
        return format(label);
    }

    return (
        <button data-action="run" onClick={handleClick}>
            {format(label)}
        </button>
    );
}

/**
 * Format the badge label.
 * @returns {string}
 */
function format(value) {
    return value.trim();
}

function evaluate(count, enabled) {
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
