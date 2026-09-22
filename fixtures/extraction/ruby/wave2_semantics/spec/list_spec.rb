RSpec.describe List, type: :model do
  before { seed_items }

  let(:app) do
    Sinatra.new do
      before { content_type :json }
    end
  end

  it { is_expected.to respond_to(:add) }
  it 'maps "" to nil', :slow do
    expect(List.parse("")).to be_nil
  end
  it "is untagged" do
    expect(List.new).to be_empty
  end

  it_behaves_like "a countable collection"

  shared_context "with a seeded list" do
    let(:seeded) { List.new([1, 2]) }
  end

  include_context "with a seeded list"
end
