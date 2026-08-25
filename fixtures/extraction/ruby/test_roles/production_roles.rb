class ReportService
  def setup
    @rows = []
  end

  def teardown
    @rows = nil
  end

  def test_connection
    true
  end
end

describe "not a suite" do
end

before do
end

after do
end

test "not a rails case" do
end

let(:not_a_fixture) { 1 }
