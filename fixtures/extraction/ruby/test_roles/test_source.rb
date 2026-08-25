RSpec.describe "ruby roles" do
  let(:order) { Order.new }
  let!(:eager_order) { Order.new }
  subject { described_class.new }

  before do
  end

  after do
  end

  around do |example|
    example.run
  end

  it "extracts an RSpec test case" do
  end

  xit "records a skipped case" do
  end

  specify "records a specify case" do
  end
end

fdescribe "focused roles" do
  fit "records a focused case" do
  end

  xspecify "records a skipped specify" do
  end
end

fcontext "focused context" do
  it "records a nested case" do
  end
end

shared_examples "a countable" do
  it "counts" do
  end
end

shared_context "with an order" do
  let(:shared_order) { Order.new }
end

def test_named_case
end

def calculate_total
  2
end

ordinary = Object.new
ordinary.it "ordinary member call" do
end

test = ExampleRunner.new
test.describe "receiver named test" do
end
