RSpec.shared_examples "a countable collection" do
  it "counts its items" do
    expect(subject.size).to eq(2)
  end
end
