local function target() end

local function parameter_shadow(target)
    target()
end

local function scope_owner()
    local function target() end
    local function nearest_caller()
        target()
    end
    nearest_caller()
end

local function hidden_owner()
    local function hidden() end
end

local function sibling_caller()
    hidden()
end

local Table = {}

function Table.owned() end

local function table_caller()
    Table.owned()
end

local function positive_caller()
    target()
end
