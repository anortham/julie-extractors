package fixture;

interface Job {
    int run();
}

class Worker implements Job {
    private final int id;
    private Map<String, List<Integer>> index;

    Worker(int id) {
        this.id = id;
    }

    @Deprecated
    public int run() {
        recordRun(id);
        return helper(id);
    }

    private static final Object lock = new Object();

    static void guardedFetch() {
        synchronized (lock) {
            fetchStatus();
        }
    }

    static void readConfig() {
        try (AutoCloseable stream = openStream()) {
            stream.close();
        } catch (Exception ignored) {
        }
    }

    private static AutoCloseable openStream() {
        return () -> {};
    }

    @SuppressWarnings("unchecked")
    static void observeAsync(Runnable task) {
        Runnable wrapped = () -> task.run();
        wrapped.run();
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

    static int evaluate(int count, boolean enabled) {
        int total = 0;
        if (enabled) {
            for (int i = 0; i < count; i++) {
                total += i;
            }
        }
        return total;
    }
}
