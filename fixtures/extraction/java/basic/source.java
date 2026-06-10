package fixture;

interface Job {
    int run();
}

class Worker implements Job {
    private final int id;

    Worker(int id) {
        this.id = id;
    }

    @Deprecated
    public int run() {
        recordRun(id);
        return helper(id);
    }

    /**
     * Increments a worker id.
     *
     * @param value the worker id
     * @return the incremented id
     */
    private static int helper(int value) {
        return value + 1;
    }

    /** Emits a worker-run marker for observability hooks. */
    private static void recordRun(int id) {
        observeRun("worker-run", id);
    }

    /** Records a named worker event for downstream hooks. */
    private static void observeRun(String event, int id) {
    }

    /** Checks the worker service health endpoint. */
    static void fetchStatus() {
        fetchUrl("https://api.example.com/workers/status");
    }

    private static void fetchUrl(String url) {
    }
}
