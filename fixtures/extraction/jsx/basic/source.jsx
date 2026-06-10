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
