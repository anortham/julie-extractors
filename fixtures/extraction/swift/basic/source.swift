protocol Job {
    func run() -> Int
}

struct Worker: Job {
    let id: Int

    func run() -> Int {
        recordRun(id)
        return helper(id)
    }
}

/// Increments a worker id.
func helper(_ value: Int) -> Int {
    value + 1
}

/// Emits a worker-run marker for observability hooks.
func recordRun(_ id: Int) {
    observeRun("worker-run", id: id)
}

/// Records a named worker event for downstream hooks.
func observeRun(_ event: String, id: Int) {
}

/// Checks the worker service health endpoint.
func fetchStatus() {
    fetchUrl("https://api.example.com/workers/status")
}

func fetchUrl(_ url: String) {
}
