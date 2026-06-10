protocol Job {
    func run() -> Int
}

@MainActor
struct Worker: Job {
    let id: Int

    func run() -> Int {
        recordRun(id)
        return helper(id)
    }
}

@available(iOS 17.0, *)
extension Worker {
    @Published var status: String = "ready"
}

@available(*, deprecated, message: "use ModernHandler")
typealias LegacyHandler = () -> Void

enum WorkerStatus {
    @available(*, deprecated)
    case legacy
    case current
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
@available(iOS 13.0, *)
func fetchStatus() {
    fetchUrl("https://api.example.com/workers/status")
}

func fetchUrl(_ url: String) {
}

func evaluate(_ count: Int, enabled: Bool) -> Int {
    var total = 0
    if enabled {
        for i in 0..<count {
            total += i
        }
    }
    return total
}
