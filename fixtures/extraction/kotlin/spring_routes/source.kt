import org.springframework.web.bind.annotation.*

@RestController
@RequestMapping("/api")
class UserController {
    @GetMapping("/users/{id}")
    fun getUser(): String = "user"

    @PostMapping("/users")
    fun createUser(): String = "created"

    @GetMapping(["/users", "/people"])
    fun listUsers(): String = "list"

    @RequestMapping(value = ["/search/{term}"], method = [RequestMethod.GET, RequestMethod.POST])
    fun search(): String = "search"

    @GetMapping
    fun index(): String = "root"

    // Silent (M2): interpolation and concatenation are not static routes.
    @GetMapping("$base/dynamic")
    fun interpolated(): String = "nope"

    @DeleteMapping("/users/" + suffix)
    fun deleteUser(): String = "nope"
}

@RestController
object HealthController {
    @GetMapping("/healthz")
    fun health(): String = "ok"
}
