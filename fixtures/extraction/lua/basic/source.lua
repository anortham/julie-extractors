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
    return self:missing_wave2()
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

return Worker
