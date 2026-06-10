-- Phase 4a fixture: cross-module call. `fn` is defined in another file
-- required via `require("other")`. The lua extractor must emit a
-- StructuredPendingRelationship with target.terminal_name="fn" (and ideally
-- target.import_context="other"). The intra-file local_helper resolves.

local other = require("other")

local function local_helper()
    return 42
end

local function entry()
    local total = 0
    if local_helper() > 0 then
        for i = 1, 2 do
            total = total + i
        end
    end
    other.fn()
    return total + local_helper()
end

return entry
