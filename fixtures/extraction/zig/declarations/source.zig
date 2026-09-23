//! Geometry declarations for the wave-2 Zig fixture.
const std = @import("std");

/// Token kinds.
pub const Tag = enum {
    /// An identifier.
    identifier,
    eof,
};

pub const Level = enum(u8) { debug = 0, info = 1 };

/// Parse failures.
pub const ParseError = error{
    UnexpectedEof,
    InvalidToken,
};

pub const Shape = struct {
    //! A shape with a width.
    const Self = @This();

    width: u32,
    next: ?*Self,

    pub fn grow(self: *Self, by: u32) void {
        self.width += by;
    }

    pub fn double(self: *Self) void {
        self.grow(self.width);
    }
};

/// Opaque C handle.
pub const Handle = opaque {};

pub const Window = opaque {
    pub fn close(self: *Window) void {
        destroy(self);
    }
};

const Empty = struct {};

const Handler = fn (u32) void;
const default_handler: *const fn (u32) void = &noop;

var counter: u32 = 0;
/// The max size.
pub const max_size: usize = 64;
const maybe: ?u32 = null;
const aligned: u32 align(8) = 0;
export var exported: u32 = 0;

extern fn destroy(handle: *Window) void;
pub extern "c" fn printf(format: [*:0]const u8, ...) c_int;
fn callconvFn() callconv(.C) void {}
noinline fn slow() void {}
fn noop(value: u32) void {
    _ = value;
}

fn pick(debug: bool) Level {
    return if (debug) .debug else .info;
}

fn parse(text: []const u8) ParseError!Tag {
    if (text.len == 0) return error.UnexpectedEof;
    return .identifier;
}

fn shapes(items: []Shape, fixed: [4]Shape) ?Shape {
    _ = fixed;
    const first = Shape{ .width = 1, .next = null };
    _ = first;
    return items[0];
}

fn sizes(n: u64) void {
    std.debug.print("{d} {s}\n", .{ @as(u32, @intCast(n)), @embedFile("data.txt") });
}

const Wheel = struct { radius: u32 };
const Car = struct {
    spare: Wheel,
    wheels: [4]Wheel,
};
