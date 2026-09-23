package com.acme

import java.sql.Connection

package object util {
  type Millis = Long
  def now(): Long = System.currentTimeMillis()
}

package http {
  class Router
}

@deprecated("use NewApi", "2.0")
trait OldApi {
  val label: String
  var alias: String
}

/** Planets. */
@SerialVersionUID(1L)
enum Planet(val mass: Double, radius: Double) {
  /** Closest to the sun. */
  @deprecated("gone", "1.0") case Mercury extends Planet(3.3e23, 2.4e6)
  case Custom(name: String, m: Double) extends Planet(m, 1.0)
}

class Service(repo: Repo, val name: String)(implicit ec: ExecutionContext) {
  @volatile var counter: Int = 0
  val (host, port) = ("localhost", 8080)
  private val x, y: Int = 2
  private val store: scala.collection.mutable.Map[String, List[Int]] = null

  def this() = this(null, "default")(null)

  @throws[java.io.IOException]
  def run(): Unit = {
    val local = repo plus name
    val doubled = List(1, 2) map double filter positive
  }

  @scala.annotation.tailrec
  final def loop(n: Int): Int = if (n <= 0) 0 else loop(n - 1)

  def double(i: Int): Int = i * 2
  def positive(i: Int): Boolean = i > 0
}

trait Codec[A]
class Money(val amount: BigDecimal) extends Ordered[Money]
object Money extends Codec[Money] with play.api.libs.json.Reads[Money]

trait Show[A]
case class Shape(r: Double)

given intShow: Show[Int] with
  def show(a: Int): String = a.toString

given Show[List[String]] with
  def show(a: List[String]): String = a.mkString

extension (s: Shape)
  def area: Double = s.r * s.r

object Dao {
  def byId(id: Long) = sql"select id, name from users where id = $id".query[User]
  def fetch(id: Long) = requests.get(s"https://api.example.com/users/$id")
  def load(ws: WSClient) = ws.url("https://api.example.com/ws").get()
}
