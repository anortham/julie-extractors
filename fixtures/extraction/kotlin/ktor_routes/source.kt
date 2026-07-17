import io.ktor.server.application.*
import io.ktor.server.routing.*
import io.ktor.server.response.*
import io.ktor.client.request.*

fun Application.module() {
    routing {
        get("/users/{id}") {
            call.respondText("ok")
        }
        post("/users") {
            call.respondText("created")
        }
        delete("/users/{id}") {
            call.respondText("gone")
        }

        // Nested route{} prefixes join into each emitted route template.
        route("/api") {
            get("/status") {
                call.respondText("up")
            }
        }

        // Silent (M2): interpolated / concatenated paths emit nothing.
        get("/users/$id") {
            call.respondText("nope")
        }
        get("/users/" + id) {
            call.respondText("nope")
        }
    }

    // Outside routing{} — silent for ktor.route.v1.
    get("/orphan") {
        call.respondText("nope")
    }

    // navigation_expression callees — silent for ktor.route.v1.
    client.get("/elsewhere")
    map.get("/not-a-route")
}
