abstract class Job {
  int run();
}

class Worker extends Job {
  final int id;

  Worker(this.id);

  @override
  int run() {
    recordRun(id);
    return helper(id);
  }

  Future<int> loadRemote() async {
    return await helper(id);
  }
}

/// Increments a worker id.
int helper(int value) {
  return value + 1;
}

/// Emits a worker-run marker for observability hooks.
void recordRun(int id) {
  observeRun("worker-run", id);
}

/// Records a named worker event for downstream hooks.
void observeRun(String event, int id) {
}

/// Checks the worker service health endpoint.
void fetchStatus() {
  fetchUrl("https://api.example.com/workers/status");
}

void fetchUrl(String url) {
}

int evaluate(int count, bool enabled) {
  var total = 0;
  if (enabled) {
    for (var i = 0; i < count; i++) {
      total += i;
    }
  }
  return total;
}
