const std = @import("std");
const types = @import("types.zig");

pub const IpcError = error{
    ConnectionClosed,
    MissingId,
    MissingErrorCode,
    MissingErrorMessage,
    SubprocessCrashed,
};

pub const Client = struct {
    allocator: std.mem.Allocator,
    process: std.process.Child,
    stdin: std.fs.File.Writer,
    stdout: std.fs.File.Reader,
    next_id: u64,
    alive: bool,

    pub fn init(allocator: std.mem.Allocator, binary_path: []const u8) !Client {
        var child = std.process.Child.init(&.{ binary_path }, allocator);
        child.stdin_behavior = .Pipe;
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Inherit;

        try child.spawn();

        return .{
            .allocator = allocator,
            .process = child,
            .stdin = child.stdin.?.writer(),
            .stdout = child.stdout.?.reader(),
            .next_id = 1,
            .alive = true,
        };
    }

    pub fn deinit(self: *Client) void {
        if (self.alive) {
            self.call(self.allocator, "shutdown", std.json.Value{ .null = {} }) catch {};
        }
        self.alive = false;
        _ = self.process.wait() catch {};
    }

    pub fn call(self: *Client, allocator: std.mem.Allocator, method: []const u8, params: std.json.Value) !std.json.Parsed(std.json.Value) {
        if (!self.alive) return error.ConnectionClosed;

        const id = self.next_id;
        self.next_id += 1;

        const req_str = try buildRequest(allocator, id, method, params);
        defer allocator.free(req_str);

        self.stdin.writeAll(req_str) catch {
            self.alive = false;
            _ = self.process.wait() catch {};
            return error.ConnectionClosed;
        };

        var line_buf: [65536]u8 = undefined;
        const line = self.stdout.readUntilDelimiterOrEof(&line_buf, '\n') catch |err| {
            self.alive = false;
            _ = self.process.wait() catch {};
            return switch (err) {
                error.EndOfStream => error.ConnectionClosed,
                else => |e| e,
            };
        } orelse {
            self.alive = false;
            return error.ConnectionClosed;
        };

        var parsed = try std.json.parseFromSlice(std.json.Value, allocator, line, .{});
        errdefer parsed.deinit();

        const root = parsed.value;
        _ = root.object.get("id") orelse return error.MissingId;

        const error_val = root.object.get("error");
        if (error_val) |ev| {
            _ = ev.object.get("code") orelse return error.MissingErrorCode;
            _ = ev.object.get("message") orelse return error.MissingErrorMessage;
            return error.SubprocessCrashed;
        }

        return parsed;
    }
};

fn buildRequest(allocator: std.mem.Allocator, id: u64, method: []const u8, params: std.json.Value) ![]u8 {
    var obj = std.json.ObjectMap.init(allocator);
    errdefer obj.deinit();

    try obj.put("id", std.json.Value{ .integer = @intCast(id) });
    try obj.put("method", std.json.Value{ .string = method });
    try obj.put("params", params);

    var buf = std.ArrayList(u8).init(allocator);
    errdefer buf.deinit();

    try std.json.stringify(std.json.Value{ .object = obj }, .{}, buf.writer());
    try buf.append('\n');

    return buf.toOwnedSlice();
}

test "ipc: build request json" {
    const req = try buildRequest(std.testing.allocator, 1, "scan", std.json.Value{ .null = {} });
    defer std.testing.allocator.free(req);

    try std.testing.expect(std.mem.containsAtLeast(u8, req, 1, "\"id\""));
    try std.testing.expect(std.mem.containsAtLeast(u8, req, 1, "\"method\""));
    try std.testing.expect(std.mem.containsAtLeast(u8, req, 1, "scan"));
}

test "ipc: build request with params" {
    var obj = std.json.ObjectMap.init(std.testing.allocator);
    defer obj.deinit();
    try obj.put("hash", std.json.Value{ .string = "sha256-abc" });

    const req = try buildRequest(std.testing.allocator, 2, "verify", std.json.Value{ .object = obj });
    defer std.testing.allocator.free(req);

    try std.testing.expect(std.mem.containsAtLeast(u8, req, 1, "sha256-abc"));
}
