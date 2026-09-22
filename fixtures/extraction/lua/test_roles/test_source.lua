describe("lua roles", function()
  before_each(function()
  end)

  it("extracts a busted test case", function()
  end)

  test("aliases a test case", function()
  end)

  pending("waits for a fix")

  strict_teardown(function()
  end)

  insulate("isolated block", function()
    spec("runs in isolation", function()
    end)
  end)
end)

TestCalc = {}

function TestCalc:setUp()
end

function TestCalc:tearDown()
end

function TestCalc:testAdd()
end

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
