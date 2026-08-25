require "test_helper"

class OrderTest < ActiveSupport::TestCase
  setup do
    @order = Order.new
  end

  teardown do
    @order = nil
  end

  test "computes the order total" do
    assert_equal 3, @order.total
  end

  def helper_order
    Order.new
  end
end

class CheckoutFlowTest < ActionDispatch::IntegrationTest
  test "posts a checkout" do
    post checkout_url
    assert_response :success
  end
end
