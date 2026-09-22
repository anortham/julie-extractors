.onLoad <- function(libname, pkgname) {
  .register_hooks()
}
.register_hooks <- function() invisible(NULL)
area <- function(shape) UseMethod("area")
area.circle <- function(shape) pi * shape$r^2
print.money <- function(x, ...) cat(format(x))
load.config <- function(path) yaml::read_yaml(path)
`%+%` <- function(a, b) paste0(a, b)

Animal <- R6::R6Class("Animal",
  public = list(
    speak = function() private$log_it("speak"),
    add = function(x) {
      total <- length(self$items) + 1
      fmt <- function(v) format(v)
      self$name <- fmt(x)
      self$size()
    },
    size = function() 1
  ),
  private = list(log_it = function(msg) message(msg))
)
Dog <- R6::R6Class("Dog", inherit = Animal, public = list(bark = function() super$speak()))
Cat <- R6::R6Class("Cat", inherit = pets::Pet)

Stack <- setRefClass("Stack", contains = "Container", fields = list(items = "list"),
  methods = list(push = function(x) {
    n <- length(items)
    items <<- c(items, x)
  }))

setClass("Shape", representation("VIRTUAL"))
setClass("Circle", contains = "Shape", slots = c(r = "numeric"))
setClass(Class = "Square", representation("Shape", side = "numeric"))
setMethod("area", signature(shape = "Circle"), function(shape) pi * shape@r^2)
setMethod(f = "show", signature = "Square", definition = function(object) cat("sq"))
setReplaceMethod("side", "Square", function(x, value) {
  x@side <- value
  x
})

cache <- new.env()
cache$items <- list()
cache[["key"]] <- 1
env$handler <- function(req) respond(req)

clean <- function(df) df
pipeline <- function(df, cfg) {
  df %>%
    clean() %>%
    external_step() %>%
    ext_bare
  cfg$db$host
  "a" %+% "b"
  purrr::map(df, mypkg::transform)
}

result <- clean(data.frame())
config <- yaml::read_yaml("config.yml")
w <- Dog$new()
