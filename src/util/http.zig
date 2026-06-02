const std = @import("std");

pub const Response = struct {
    status: std.http.Status,
    body: []u8,

    pub fn deinit(self: *Response, allocator: std.mem.Allocator) void {
        allocator.free(self.body);
    }
};

pub const RequestOptions = struct {
    method: std.http.Method = .GET,
    extra_headers: []const std.http.Header = &.{},
    max_append_size: ?usize = null,
};

pub const HttpClient = struct {
    allocator: std.mem.Allocator,
    client: std.http.Client,

    pub fn init(allocator: std.mem.Allocator) HttpClient {
        return .{
            .allocator = allocator,
            .client = .{ .allocator = allocator },
        };
    }

    pub fn deinit(self: *HttpClient) void {
        self.client.deinit();
    }

    pub fn get(self: *HttpClient, url: []const u8) !Response {
        return self.fetch(url, .{});
    }

    pub fn fetch(self: *HttpClient, url: []const u8, opts: RequestOptions) !Response {
        var body = std.ArrayList(u8).init(self.allocator);
        defer body.deinit();

        const result = try self.client.fetch(.{
            .location = .{ .url = url },
            .method = opts.method,
            .response_storage = .{ .dynamic = &body },
            .extra_headers = opts.extra_headers,
            .max_append_size = opts.max_append_size,
        });

        if (result.status.class() != .success) return error.StatusNotOk;

        return .{
            .status = result.status,
            .body = try body.toOwnedSlice(),
        };
    }
};

test "http: get example.com" {
    var client = HttpClient.init(std.testing.allocator);
    defer client.deinit();

    var resp = try client.get("https://example.com");
    defer resp.deinit(std.testing.allocator);

    try std.testing.expect(resp.body.len > 0);
    try std.testing.expect(std.mem.containsAtLeast(u8, resp.body, 1, "Example Domain"));
}

test "http: 404 returns StatusNotOk" {
    var client = HttpClient.init(std.testing.allocator);
    defer client.deinit();

    try std.testing.expectError(error.StatusNotOk, client.get("https://example.com/nonexistent"));
}

test "http: invalid url" {
    var client = HttpClient.init(std.testing.allocator);
    defer client.deinit();

    try std.testing.expectError(error.UnexpectedCharacter, client.get("not a url"));
}
