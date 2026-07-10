defmodule CalculatorTest do
  use ExUnit.Case

  setup do
    {:ok, value: 2}
  end

  describe "addition" do
    test "adds two numbers", %{value: value} do
      assert value + 2 == 4
    end
  end

  def verify_addition(value) do
    value + 2 == 4
  end
end
