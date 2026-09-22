package main

import (
	"net/http"
	"github.com/gin-gonic/gin"
	"github.com/labstack/echo/v4"
)

func routes() {
	http.HandleFunc("GET /users/{id}", show)
	mux := http.NewServeMux()
	mux.Handle("POST /files/{path...}", handler)

	r := gin.Default()
	api := r.Group("/api")
	api.GET("/users/:id", showGin)

	http.HandleFunc("admin.example.com/", adminHost)
	http.HandleFunc("GET reports.example.com/reports/{id}", hostReport)
	r.Any("/ping", anyGin)
	r.Handle("PUT", "/manual", manualGin)
	nested := api.Group("/nested")
	nested.GET("/deep/:id", deepGin)
	dynamic := r.Group(prefixFor("tenant"))
	dynamic.GET("/records/:id", recordsGin)
	apiRouter.GET("/silent", handler)

	e := echo.New()
	v1 := e.Group("/v1")
	v1.POST("/items/:id", createEcho)
	e.Any("/anything", anyEcho)
}

type server struct {
	mux *http.ServeMux
}

func registerGin(router *gin.Engine, group *gin.RouterGroup) {
	v2 := router.Group("/v2")
	v2.GET("/orders/:id", showOrder)
	group.DELETE("/orders/:id", deleteOrder)
}

func registerEcho(app *echo.Echo) {
	app.GET("/health", health)
}

func (s *server) register() {
	s.mux.HandleFunc("GET /metrics", metrics)
}

func clients() {
	http.Get("https://api.example.com/users")
	http.NewRequest("PATCH", "/users/1", nil)
}
