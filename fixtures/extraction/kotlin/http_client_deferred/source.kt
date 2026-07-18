// Deferred Kotlin HTTP clients: OkHttp, Retrofit, WebClient, RestTemplate.
// Backs closure of kotlin.http_client.deferred.

import okhttp3.Request
import retrofit2.http.GET
import retrofit2.http.POST
import org.springframework.web.reactive.function.client.WebClient
import org.springframework.web.client.RestTemplate

interface DeferredApi {
    @GET("/users")
    suspend fun users(): List<String>

    @POST("/items")
    suspend fun create(): Unit
}

fun deferredClients(web: WebClient, rest: RestTemplate, body: okhttp3.RequestBody, url: String) {
    Request.Builder().url("https://api.example.com/health").build()
    Request.Builder().url("/items").post(body).build()
    web.get().uri("/web/users").retrieve()
    web.delete().uri("https://api.example.com/web/1").retrieve()
    rest.getForObject("/legacy/users", String::class.java)
    rest.postForObject("/legacy/items", body, String::class.java)

    // Silent dynamics.
    Request.Builder().url(url).build()
    web.get().uri(url).retrieve()
    rest.getForObject(url, String::class.java)
}
