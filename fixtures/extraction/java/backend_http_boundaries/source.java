import java.net.URI;
import java.net.http.HttpRequest;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api")
class UserController {
    @GetMapping("/users/{id}")
    public User getUser() { return null; }

    @RequestMapping(method = {RequestMethod.GET, RequestMethod.POST}, path = "/search/{term}")
    public User search() { return null; }

    void callClient() {
        HttpRequest req = HttpRequest.newBuilder(URI.create("https://api.example.com/users")).build();
        HttpRequest post = HttpRequest.newBuilder().uri(URI.create("/items")).POST(body).build();
    }
}
