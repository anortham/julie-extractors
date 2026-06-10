package fixture

interface Job {
    fun run(): Int
}

class Worker(private val id: Int) : Job {
    override fun run(): Int {
        recordRun(id)
        return helper(id)
    }

    /**
     * Increments a worker id.
     */
    private fun helper(value: Int): Int {
        return value + 1
    }

    /** Emits a worker-run marker for observability hooks. */
    private fun recordRun(id: Int) {
        observeRun("worker-run", id)
    }

    /** Records a named worker event for downstream hooks. */
    private fun observeRun(event: String, id: Int) {
    }

    /** Checks the worker service health endpoint. */
    fun fetchStatus() {
        fetchUrl("https://api.example.com/workers/status")
    }

    private fun fetchUrl(url: String) {
    }
}
