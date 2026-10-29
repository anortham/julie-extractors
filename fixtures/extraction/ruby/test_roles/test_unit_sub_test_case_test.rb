require "test/unit"

class StackTest < Test::Unit::TestCase
  sub_test_case "push" do
    setup do
      @stack = []
    end

    test "adds one item" do
      @stack.push(1)
      assert_equal 1, @stack.size
    end

    def test_adds_two_items
      @stack.push(1, 2)
      assert_equal 2, @stack.size
    end

    sub_test_case "when full" do
      test "raises" do
        assert true
      end
    end
  end

  test "starts empty" do
    assert_equal 0, [].size
  end
end
