protocol Job {
    func run() -> Int
}

struct Worker: Job {
    let id: Int

    func run() -> Int {
        helper(id)
    }
}

/// Increments a worker id.
func helper(_ value: Int) -> Int {
    value + 1
}
