local json = require("json")

local Worker = {}
Worker.__index = Worker

--- Increment a worker id.
local function helper(value)
    return value + 1
end

local function run_worker(worker)
    return helper(worker.id)
end

function Worker:new(id)
    return setmetatable({ id = id }, Worker)
end

function Worker:run()
    return helper(self.id)
end

function Worker:log()
    self:missing_wave2()
    return self:run()
end

local function evaluate(count, enabled)
    local total = 0
    if enabled then
        for i = 1, count do
            total = total + i
        end
    elseif count > 0 then
        total = count > 10 and 1 or 0
    end
    return total
end

local co = coroutine.create(function()
    return 1
end)

local worker = Worker.new(1)
local boxed = setmetatable({}, Worker)

---@brief
--- File section header, separated from the next declaration.

local class = require("middleclass")
local Base = require("base")

---@class Shape
local Shape = {}
Shape.__index = Shape

--- Build a shape.
---@param kind string
---@param size? integer
---@return Shape
function Shape.new(kind, size)
    local self = setmetatable({}, Shape)
    self.kind = kind
    self.size = size or 1
    return self
end

---@nodiscard
function Shape:grow(step)
    self.size = self.size + step
    return Shape.new(self.kind, self.size)
end

---@class Square : Shape
---@deprecated
local Square = setmetatable({}, { __index = Shape })

local Animal = class("Animal")
local Dog = class("Dog", Animal)
local Remote = setmetatable({}, Base)
Remote.__index = Remote

Account = { balance = 0 }
function Account:new(o)
    o = o or {}
    setmetatable(o, self)
    self.__index = self
    return o
end
SpecialAccount = Account:new()
function SpecialAccount:withdraw(v)
    self.balance = self.balance - v
end

local settings = {
    --- Enabled flag.
    ---@deprecated
    enabled = true,
    nested = { width = 80 },
}

local is_even, is_odd
function is_even(n)
    return n == 0 or is_odd(n - 1)
end
function is_odd(n)
    return n ~= 0 and is_even(n - 1)
end

---@deprecated
local function render(canvas, list, db)
    ---@type Shape
    local shape = Shape.new("box")
    local acc = Account:new({ balance = 100 })
    local rex = Dog("rex")
    local weak = setmetatable({}, { __mode = "k" })
    canvas:clear()
    list.items:sort()
    shape:grow(1)
    vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = 1, desc = "Close" })
    require("lazy.core.config").setup({})
    db:query("SELECT id FROM users")
    os.execute("rm -rf /tmp/cache")
    return acc, rex, weak
end

return Worker
