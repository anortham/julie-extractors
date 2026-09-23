import io.ktor.client.HttpClient
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.request.delete
import org.springframework.web.client.RestTemplate
import retrofit2.http.HTTP

suspend fun syncUsers(client: HttpClient, id: String): String {
    client.get("https://api.example.com/users")
    client.post("/items")
    client.delete("/users/1")

    // Silent (M2): interpolated / concatenated URLs are not static literals.
    client.get("$base/users")
    client.get("/users/" + id)
    return "ok"
}

// Typed class-body properties prove the client receiver.
class Gateway {
    private lateinit var rest: RestTemplate
    private lateinit var ktor: HttpClient
    private lateinit var other: RestTemplateFactory

    fun legacyUser() = rest.getForObject("https://api.example.com/legacy", String::class.java)
    suspend fun status() = ktor.get("https://api.example.com/status")

    // Silent: the property type is not a known client.
    fun unrelated() = other.getForObject("https://api.example.com/other", String::class.java)
}

// Retrofit @HTTP with a static method and path.
interface CustomVerbApi {
    @HTTP(method = "DELETE", path = "users/{id}", hasBody = true)
    fun remove(): Call<Unit>

    // Silent: a non-constant or unknown method.
    @HTTP(method = VERB, path = "dynamic")
    fun dynamic(): Call<Unit>
}
