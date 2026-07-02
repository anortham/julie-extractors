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

	e := echo.New()
	v1 := e.Group("/v1")
	v1.POST("/items/:id", createEcho)
}

func clients() {
	http.Get("https://api.example.com/users")
	http.NewRequest("PATCH", "/users/1", nil)
}
