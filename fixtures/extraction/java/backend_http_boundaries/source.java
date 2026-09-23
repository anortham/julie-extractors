import java.net.URI;
import java.net.http.HttpRequest;
import org.springframework.web.bind.annotation.*;
import org.springframework.web.client.RestTemplate;
import org.springframework.web.reactive.function.client.WebClient;
import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.http.HttpMethod;
import jakarta.ws.rs.*;

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

@RestController
class HealthController {
    @GetMapping(value = "/healthz", produces = "application/json")
    public String health() { return "ok"; }

    @RequestMapping("/legacy")
    public String legacy() { return "ok"; }
}

@RestController
@RequestMapping("/api/orders")
class OrderController {
    static class OrderDto { String id; }

    @GetMapping
    public OrderDto list() { return null; }

    @GetMapping("/inline") public String inline() { return ""; }

    public String helper() { return ""; }

    @ResponseStatus(HttpStatus.CREATED) @PostMapping("/{id}/items") public String addItem(long id) { return ""; }

    @org.springframework.web.bind.annotation.DeleteMapping("/{id}")
    public void remove(long id) {}
}

@FeignClient(name = "inventory", path = "/inventory")
interface InventoryClient {
    @GetMapping("/stock/{sku}")
    Stock stock(@PathVariable("sku") String sku);
}

class OutboundClients {
    private final RestTemplate rest = new RestTemplate();
    private final WebClient web = WebClient.create("https://api.example.com");

    String fetchUser() { return rest.getForObject("https://api.example.com/users/{id}", String.class, 1); }

    String fetchOrder() { return web.get().uri("/orders/{id}", 1).retrieve().bodyToMono(String.class).block(); }

    void replaceOrder(RestTemplate template) { template.exchange("/orders/{id}", HttpMethod.PUT, null, String.class, 1); }
}

@Path("/shipments")
class ShipmentResource {
    @GET
    @Path("/{id}")
    public Shipment get(@PathParam("id") long id) { return null; }

    @POST
    public Shipment create(Shipment shipment) { return shipment; }

    @Path("{id}/events")
    public EventsResource events() { return null; }
}
