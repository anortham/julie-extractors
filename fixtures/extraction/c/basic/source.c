/**
 * Worker state passed through the C helper pipeline.
 */
typedef struct Worker {
    int id;
} Worker;

int helper(int value);
void worker_log(const char *message);

/**
 * Run the worker through the helper pipeline.
 */
[[nodiscard]]
int worker_run(Worker *worker) {
    worker_log("worker-run");
    return helper(worker->id);
}

int helper(int value) {
    return value + 1;
}

int evaluate(int count, int enabled) {
    int total = 0;
    if (enabled) {
        for (int i = 0; i < count; i++) {
            total += i;
        }
    }
    return total;
}

struct bar;

struct node {
    struct bar *next;
};

void receive_facts(struct Worker *x, const char *s, int n) {
    struct Worker *p = x;
    int buf[8];
}

void handler(void (*cb)(int)) {
}
