package fixture;

interface Job {
    int run();
}

class Worker implements Job {
    private final int id;

    Worker(int id) {
        this.id = id;
    }

    public int run() {
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
}
