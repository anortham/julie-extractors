.setUp <- function() {
  options(money.digits = 2)
}

.tearDown <- function() {
  options(money.digits = NULL)
}

testMoneyPrint <- function() {
  checkTrue(is.function(print))
}

test.money_creation <- function() {
  checkEquals(format_money(1), "1.00")
}

format_money <- function(x) {
  format(x, nsmall = 2)
}
