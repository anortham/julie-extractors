describe("r roles", {
  it("extracts a testthat BDD case", {
    expect_true(TRUE)
  })
})

test_that("extracts a testthat case", {
  expect_equal(1 + 1, 2)
})

test.named.case <- function() {
  TRUE
}

calculate_total <- function() {
  2
}

describe.default("ordinary dotted call", {
  TRUE
})
