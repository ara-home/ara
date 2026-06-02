const std = @import("std");

pub const RpcError = struct {
    code: i32,
    message: []const u8,
};

pub const Request = struct {
    id: u64,
    method: []const u8,
    params: std.json.Value,
};

pub const Response = struct {
    id: u64,
    result: ?std.json.Value,
    @"error": ?RpcError,
};

pub const Method = enum {
    scan,
    verify,
    audit,
    shutdown,
    analyze,

    pub fn toString(self: Method) []const u8 {
        return @tagName(self);
    }
};

test "Method.toString returns tag name" {
    try std.testing.expectEqualStrings("scan", Method.scan.toString());
    try std.testing.expectEqualStrings("verify", Method.verify.toString());
    try std.testing.expectEqualStrings("audit", Method.audit.toString());
    try std.testing.expectEqualStrings("shutdown", Method.shutdown.toString());
    try std.testing.expectEqualStrings("analyze", Method.analyze.toString());
}

test "Request JSON roundtrip" {
    var obj = std.json.ObjectMap.init(std.testing.allocator);
    defer obj.deinit();
    try obj.put("hash", std.json.Value{ .string = "sha256-abc" });

    const req = Request{
        .id = 7,
        .method = "scan",
        .params = std.json.Value{ .object = obj },
    };

    var buf = std.ArrayList(u8).init(std.testing.allocator);
    defer buf.deinit();
    try std.json.stringify(req, .{}, buf.writer());

    const json = buf.items;
    try std.testing.expect(std.mem.containsAtLeast(u8, json, 1, "7"));
    try std.testing.expect(std.mem.containsAtLeast(u8, json, 1, "scan"));
    try std.testing.expect(std.mem.containsAtLeast(u8, json, 1, "sha256-abc"));
}

test "RpcError creation" {
    const err = RpcError{ .code = -1, .message = "analysis failed" };
    try std.testing.expectEqual(@as(i32, -1), err.code);
    try std.testing.expectEqualStrings("analysis failed", err.message);
}
