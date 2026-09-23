const std = @import("std");
const httpz = @import("httpz");

pub fn serve(server: *httpz.Server(void)) !void {
    var router = try server.router(.{});
    router.get("/api/users/:id", getUser, .{});
    router.all("/health", health, .{});
    var admin = router.group("/admin", .{});
    admin.post("/users", createUser, .{});
    admin.delete("/users/:id", deleteUser, .{});
}

fn getUser(req: *httpz.Request, res: *httpz.Response) !void {
    _ = req;
    _ = res;
}

fn createUser(req: *httpz.Request, res: *httpz.Response) !void {
    _ = req;
    _ = res;
}

fn deleteUser(req: *httpz.Request, res: *httpz.Response) !void {
    _ = req;
    _ = res;
}

fn health(req: *httpz.Request, res: *httpz.Response) !void {
    _ = req;
    _ = res;
}

pub fn ping(allocator: std.mem.Allocator) !void {
    var client = std.http.Client{ .allocator = allocator };
    defer client.deinit();
    _ = try client.fetch(.{ .location = .{ .url = "https://api.example.com/health" }, .method = .GET });
    _ = try client.fetch(.{ .location = .{ .url = "https://api.example.com/events" }, .payload = "{}" });
    const uri = try std.Uri.parse("https://api.example.com/items/1");
    var buf: [1024]u8 = undefined;
    var req = try client.open(.DELETE, uri, .{ .server_header_buffer = &buf });
    defer req.deinit();
}
