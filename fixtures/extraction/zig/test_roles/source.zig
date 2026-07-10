const std = @import("std");

test "addition works" {
    try std.testing.expect(2 + 2 == 4);
}

fn helperNamedLikeATest() void {}
