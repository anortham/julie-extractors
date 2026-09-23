const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const httpz = b.dependency("httpz", .{ .target = target, .optimize = optimize });

    const core = b.addModule("core", .{ .root_source_file = b.path("src/core.zig") });

    const exe = b.addExecutable(.{
        .name = "app",
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
    });
    exe.root_module.addImport("httpz", httpz.module("httpz"));
    exe.root_module.addImport("core", core);
    b.installArtifact(exe);

    const tests = b.addTest(.{
        .root_module = b.createModule(.{ .root_source_file = b.path("src/tests.zig") }),
    });

    const run_step = b.step("run", "Run the app");
    run_step.dependOn(&b.addRunArtifact(exe).step);
    const test_step = b.step("test", "Run unit tests");
    test_step.dependOn(&b.addRunArtifact(tests).step);
}
