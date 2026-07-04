import io.ktor.client.HttpClient
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.request.delete

suspend fun syncUsers(client: HttpClient, id: String): String {
    client.get("https://api.example.com/users")
    client.post("/items")
    client.delete("/users/1")

    // Silent (M2): interpolated / concatenated URLs are not static literals.
    client.get("$base/users")
    client.get("/users/" + id)
    return "ok"
}
