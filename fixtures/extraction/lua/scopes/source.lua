local util = require "stack.util"

local function helper(value)
    return value
end

local Player = {}
Player.__index = Player

function Player:update(dt)
    self.x = clamp(self.x + dt)
    physics.step(dt)
end

local Enemy = {}
Enemy.__index = Enemy

function Enemy:update(dt)
    self.y = clamp(self.y - dt)
    return helper(dt)
end

local M = {}
M.config = { run = function(opts) return util.run(opts) end }
M.config.timeout = 30

M.mul = function(a, b)
    local r = util.scale(a, b)
    return helper(r)
end

function M.nested.deep(x)
    return util.deep(x)
end

function M.nested.inner:call(y)
    return self:other(y)
end

local count

local function bump(n)
    n = n + 1
    count = (count or 0) + n
    registry = n
    registry = registry + 1
    return n
end

return { M = M, bump = bump }
