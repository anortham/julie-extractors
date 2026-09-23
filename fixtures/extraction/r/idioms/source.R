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

#' A person record.
Person <- setClass("Person", representation(name = "character", age = "numeric"))
setClass("Employee", contains = "Person", slots = c(boss = "Person", salary = "numeric"))

Account <- setRefClass("Account", fields = c(balance = "numeric"))
Account$methods(
  withdraw = function(x) {
    balance <<- balance - x
  },
  report = function() cat(balance)
)

Counter <- R6::R6Class("Counter",
  public = list(total = 0, add = function(n) private$log_it(n)),
  private = list(log_it = function(n) message(n)),
  active = list(doubled = function(value) self$total * 2)
)

Shape <- S7::new_class("Shape", properties = list(label = S7::class_character))
area <- S7::new_generic("area", "shape")
S7::method(area, Shape) <- function(shape) nchar(shape@label)

#' @keywords internal
.helper <- function(x) x

loaders <- function(dir) {
  source(file.path("R", "helpers.R"))
  requireNamespace("jsonlite")
  box::use(dplyr[filter, select], app/logic/utils)
  pacman::p_load(tidyr)
  shape <- Shape(label = "box")
  for (row in dir) print(row)
  fit <- lm(mpg ~ wt, data = mtcars)
  mtcars |> subset(cyl == 4, select = mpg) |> summary()
  ggplot2::facet_wrap(~ cyl)
  area(shape)
}
