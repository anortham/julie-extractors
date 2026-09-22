package com.acme.core

private[core] class Internal {
  private[this] def secret(): Int = 1
  protected[core] def shared(): Int = secret()
  private[acme] val cfg: String = "x"
}

class Helper(x: Int)

class Base {
  def greet(): String = "hi"
}

class Child extends Base {
  override def greet(): String = super.greet() + "!"
}

object Pipeline {
  val built = compute(1)
  def load(path: String): List[String] = Nil
  def compute(x: Int): Int = x * 2
  def decode[T](s: String): T = ???
  def run(path: String, xs: List[String]): Int = {
    val lines = load(path)
    lazy val extra = compute(lines.size)
    var total = remote.fetch(path)
    val parsed = decode[Int]("1")
    val json = Json.parse[User]("{}")
    val h = new Helper(1)
    val m = new scala.collection.mutable.HashMap[String, Int]()
    val url = ConfigFactory.load().getString("app.url")
    val positive = xs.map(x => parse(x)).filter(_ > 0)
    extra + total
  }
}

class ConnectionPool {
  def testConnection(): Boolean = true
  def beforeAll(): Unit = ()
}

object Service:
  /** Doc f. */
  def f(): Int =
    val a = 1
    a + 1

  /** Doc g. */
  def g(): Int =
    f() * 2

class A:
  def x: Int = 1

/** Doc for B. */
class B
