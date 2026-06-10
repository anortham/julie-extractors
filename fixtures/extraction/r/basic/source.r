#' Increment a worker id.
#' @param value worker id
helper <- function(value) {
  value + 1
}

run_worker <- function(id) {
  helper(id)
}

evaluate <- function(count, enabled) {
  total <- 0
  if (enabled) {
    for (i in 1:count) {
      total <- total + i
    }
  } else if (count > 0) {
    total <- 1
  }
  total <- switch(count %% 3, total, total + 1, total + 2)
  total
}

Worker <- R6::R6Class(
  "Worker",
  public = list(
    id = NULL,
    initialize = function(id) {
      self$id <- id
    },
    run = function() {
      helper(self$id)
    }
  )
)
