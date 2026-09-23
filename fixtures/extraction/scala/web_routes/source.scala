import akka.http.scaladsl.server.Directives._
import org.http4s._
import play.api.libs.ws.WSClient
import sttp.client3._

class UserRoutes(service: UserService) {
  val route =
    pathPrefix("users") {
      get { path(LongNumber) { id => complete(service.find(id)) } } ~
      post { entity(as[User]) { user => complete(service.create(user)) } } ~
      path("admin" / Segment) { name => delete { complete("gone") } }
    } ~
    // Silent: a dynamic path matcher and a method with no path.
    path(dynamicPath) { get { complete("nope") } } ~
    get { complete("no path") }
}

object Http4sRoutes {
  val routes = HttpRoutes.of[IO] {
    case GET -> Root / "users" / IntVar(id) => Ok(s"user $id")
    case req @ POST -> Root / "users" => Created("ok")
    case GET -> Root / "search" :? QueryMatcher(q) => Ok(q)
    case other => NotFound()
  }
}

class Clients(ws: WSClient, other: Other) {
  def items() = requests.get("https://api.example.com/items")
  def create() = basicRequest.post(uri"https://api.example.com/post").send(backend)
  def remove() = ws.url("https://api.example.com/ws").delete()

  // Silent: an interpolated URL and an unproven receiver.
  def user(id: Int) = basicRequest.get(uri"https://api.example.com/users/$id")
  def unrelated() = other.url("https://nope.example.com").get()
}
