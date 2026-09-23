package routes

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/gofiber/fiber/v2"
	"github.com/gorilla/mux"
)

func chiRoutes() http.Handler {
	r := chi.NewRouter()
	r.Get("/users/{id}", showUser)
	r.With(authenticate).Post("/users", createUser)
	r.Method(http.MethodPut, "/users/{id}", updateUser)
	r.Route("/admin", func(admin chi.Router) {
		admin.Delete("/users/{id}", deleteUser)
	})
	r.Mount("/api", apiRouter())
	return r
}

func gorillaRoutes() *mux.Router {
	router := mux.NewRouter()
	router.HandleFunc("/items/{id}", showItem).Methods("GET")
	api := router.PathPrefix("/v1").Subrouter()
	api.HandleFunc("/items", createItem).Methods(http.MethodPost)
	return router
}

func fiberRoutes() *fiber.App {
	app := fiber.New()
	app.Get("/orders/:id", showOrder)
	v1 := app.Group("/v1")
	v1.Post("/orders", createOrder)
	return app
}

func clientCalls() {
	client := &http.Client{}
	req, _ := http.NewRequest(http.MethodDelete, "https://api.example.com/users/1", nil)
	_, _ = client.Do(req)
	_, _ = client.Get("https://api.example.com/users")
	_, _ = http.DefaultClient.Post("https://api.example.com/users", "application/json", nil)
}

func showUser(w http.ResponseWriter, r *http.Request)   {}
func createUser(w http.ResponseWriter, r *http.Request) {}
func updateUser(w http.ResponseWriter, r *http.Request) {}
func deleteUser(w http.ResponseWriter, r *http.Request) {}
func showItem(w http.ResponseWriter, r *http.Request)   {}
func createItem(w http.ResponseWriter, r *http.Request) {}
func showOrder(c *fiber.Ctx) error                      { return nil }
func createOrder(c *fiber.Ctx) error                    { return nil }
func apiRouter() http.Handler                           { return nil }
func authenticate(next http.Handler) http.Handler       { return next }
