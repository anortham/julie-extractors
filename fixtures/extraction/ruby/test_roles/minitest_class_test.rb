require "minitest/autorun"

class CalculatorTest < Minitest::Test
  def setup
    @calculator = Calculator.new
  end

  def teardown
    @calculator = nil
  end

  def test_adds_two_numbers
    assert_equal 3, @calculator.add(1, 2)
  end

  def build_operand
    1
  end
end

class LegacyCalculatorTest < Test::Unit::TestCase
  def test_subtracts_two_numbers
    assert_equal 1, 2 - 1
  end
end

class FixtureBuilder
  def setup
    @rows = []
  end

  def test_rows
    @rows
  end
end
