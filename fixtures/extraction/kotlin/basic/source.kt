package fixture

interface Job {
    fun run(): Int
}

@Singleton
object WorkerRegistry

@Suppress("UNCHECKED_CAST")
typealias WorkerCallback = (Int) -> Unit

@Deprecated("Use WorkerV2")
class Worker(
    @Suppress("UNUSED") private val id: Int,
) : Job {
    @Volatile
    var status: String = "ready"

    private val index: List<Map<String, Int>> = emptyList()

    @Deprecated("Legacy entry point")
    override fun run(): Int {
        recordRun(id)
        return helper(id)
    }

    suspend fun loadRemote(): Int {
        return helper(id)
    }

    val runner by lazy { Worker(id) }

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

    fun persist() {
        this.recordRun(id)
        this.missingWave2()
    }

    constructor(label: String) : this(label.length)
}

fun evaluate(count: Int, enabled: Boolean): Int {
    val maybe: Job? = null
    var total = 0
    if (enabled) {
        for (i in 0 until count) {
            total += i
        }
    }
    return total
}
