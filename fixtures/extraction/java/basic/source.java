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

    /** Fully-qualified static call: the terminal receiver `Worker` must stay name-visible. */
    static void auditViaQualifiedCall() {
        int latest = fixture.Worker.evaluate(2, true);
        fixture.Worker.observeRun("qualified-audit", latest);
    }

    void consume(Job[] jobs) {
        for (Job job : jobs) {
            if (job instanceof Worker bound) {
                bound.run();
            }
        }
        this.recordRun(id);
    }
}

class Supervisor extends Worker {
    Supervisor(int id) {
        super(id);
    }

    @Override
    public int run() {
        return super.run();
    }

    void combine(BinaryOperator<Integer> op) {
        BinaryOperator<Integer> sum = (left, right) -> left + right;
        sum.apply(1, 2);
    }
}

interface RetryPolicy {
    /** Maximum retry attempts. */
    int MAX_RETRIES = 3;
    java.time.Duration backoff();
}

/** Audit marker. */
@java.lang.annotation.Retention(java.lang.annotation.RetentionPolicy.RUNTIME)
@interface Audited {
    /** Audit level. */
    String level() default "info";
    int priority();
}

@Deprecated
enum LegacyMode { OFF }

record Money(@JsonProperty("amount_cents") long cents, String currency) {
    Money {
        if (cents < 0) throw new IllegalArgumentException("negative");
        validate(cents);
    }

    static void validate(long value) {}
}

class ShapeMath {
    private java.util.concurrent.atomic.AtomicInteger calls;
    private Map.Entry<String, Integer> lastEntry;

    @Audited(level = "high")
    java.util.Map<String, java.util.List<Money>> byCurrency() { return null; }

    double area(Object shape) {
        var cache = new java.util.ArrayList<String>();
        return switch (shape) {
            case Circle c -> Math.PI * c.radius() * c.radius();
            case Rect(double w, double h) -> w * h;
            default -> 0;
        };
    }
}

interface OrderRepository {
    @Query("select o from Order o where o.status = :status")
    java.util.List<Order> byStatus(String status);
}

class OrderQueries {
    long count(EntityManager em) {
        return (long) em.createNativeQuery("SELECT count(*) FROM orders").getSingleResult();
    }
}
