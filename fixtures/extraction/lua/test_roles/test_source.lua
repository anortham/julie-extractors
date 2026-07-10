describe("lua roles", function()
  before_each(function()
  end)

  it("extracts a busted test case", function()
  end)
end)

function test_named_case()
end

function calculate_total()
  return 2
end

local ordinary = {}
ordinary.it = function(_, callback)
  callback()
end
ordinary.it("ordinary member call", function()
end)
