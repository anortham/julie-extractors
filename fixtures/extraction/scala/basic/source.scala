package fixture

trait Job {
  def run(): Int
}

@deprecated("Use WorkerV2", since = "2.0")
class Worker(val id: Int) extends Job {
  @deprecated("Prefer runSync", since = "2.0")
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

  def evaluate(count: Int, enabled: Boolean): Int = {
    var total = 0
    if (enabled) {
      for (i <- 0 until count) total += i
    } else if (count > 0) {
      total = if (count > 10) 1 else 0
    }
    total
  }

  def scanPositive(items: List[Int]): List[Int] =
    for {
      item <- items
      if item > 0
    } yield item * 2
}

given Ordering[Int] = Ordering.Int

@singleton
object WorkerRegistry {
  @tracked val runs: Int = 0
}

@opaque
type WorkerId = Int

extension (value: Int)
  @inline def doubled: Int = value * 2

@deprecated("legacy", since = "1.0")
def legacyHook(): Unit = ()

class Foo

case class Payload(a: Foo)
class Query(a: Foo)

class Widget {
  def this(seed: Foo) = this()
  def ping(): Unit = {
    val typed: Foo = null
    val constructed = new Foo()
    val sameFile = Foo()
    val unknown = Unknown()
    val imported = scala.collection.mutable.ListBuffer()
    val built = build()
    this.m()
    this.missingWave2()
    other.m()
  }
  def m(): Unit = ()
  def annotate(x: Foo, xs: List[Foo]): Unit = ()
}

def build(): Int = 1
