package fixture

trait Job {
  def run(): Int
}

class Worker(val id: Int) extends Job {
  def run(): Int = {
    recordRun(id)
    helper(id)
  }

  private def helper(value: Int): Int = value + 1

  /** Emits a worker-run marker for observability hooks. */
  private def recordRun(id: Int): Unit = {
    observeRun("worker-run", id)
  }

  /** Records a named worker event for downstream hooks. */
  private def observeRun(event: String, id: Int): Unit = ()

  /** Checks the worker service health endpoint. */
  def fetchStatus(): Unit = {
    fetchUrl("https://api.example.com/workers/status")
  }

  private def fetchUrl(url: String): Unit = ()
}
