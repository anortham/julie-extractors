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

describe("calculate_total", {
  it("sums", {
    expect_equal(calculate_total(), add_one(1))
  })
})

setup({
  options(demo = TRUE)
})

teardown({
  options(demo = NULL)
})

testthat::test_that("qualified case", {
  expect_true(TRUE)
})
