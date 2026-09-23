import org.springframework.web.reactive.function.server.coRouter
import org.springframework.web.servlet.function.router

class Routes(private val handler: UserHandler) {
    fun api() = coRouter {
        "/api/users".nest {
            GET("", handler::list)
            GET("/{id}", handler::get)
        }
        GET("/health") {
            ok().bodyValueAndAwait("up")
        }
        path("/v2").nest {
            PUT("/items/{id}", handler::update)
        }
        // Silent: an interpolated nest prefix is not static.
        "$base/dyn".nest {
            GET("/hidden", handler::hidden)
        }
    }

    fun mvc() = router {
        accept(APPLICATION_JSON).nest {
            DELETE("/mvc/{id}", handler::delete)
        }
    }
}
