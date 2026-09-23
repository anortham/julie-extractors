defmodule CalculatorTest do
  use ExUnit.Case
  use ExUnitProperties
  doctest Calculator

  setup do
    {:ok, value: 2}
  end

  setup :seed

  property "addition commutes" do
    check all a <- integer(), b <- integer() do
      assert a + b == b + a
    end
  end

  defp seed(_context), do: %{seed: 1}

  describe "addition" do
    test "adds two numbers", %{value: value} do
      assert value + 2 == 4
    end
  end

  def test_helper_value, do: 2

  def verify_addition(value) do
    value + 2 == 4
  end
end
