const std = @import("std");

pub const HttpClient = struct {
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) HttpClient {
        return .{ .allocator = allocator };
    }

    pub fn get(self: *HttpClient, url: []const u8) ![]u8 {
        _ = self;
        _ = url;
        return "";
    }

    pub fn deinit(self: *HttpClient) void {
        _ = self;
    }
};

test "http: placeholder" {
    var client = HttpClient.init(std.testing.allocator);
    client.deinit();
}
