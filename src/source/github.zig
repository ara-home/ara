const std = @import("std");
const types = @import("../types.zig");
const http = @import("../util/http.zig");

pub const GithubSource = struct {
    allocator: std.mem.Allocator,
    repo: []const u8,

    pub fn init(allocator: std.mem.Allocator, repo: []const u8) !GithubSource {
        return .{ .allocator = allocator, .repo = try allocator.dupe(u8, repo) };
    }

    pub fn deinit(self: *GithubSource) void {
        self.allocator.free(self.repo);
    }

    pub fn resolve(self: GithubSource, a: std.mem.Allocator, _: []const u8) ![]const u8 {
        _ = a;
        return self.repo;
    }

    pub fn fetch(self: GithubSource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        var ver_buf: [64]u8 = undefined;
        const ver_str = try std.fmt.bufPrint(&ver_buf, "{}.{}.{}", .{ id.version.major, id.version.minor, id.version.patch });

        const url = try std.fmt.allocPrint(a, "https://api.github.com/repos/{s}/tarball/v{s}", .{ self.repo, ver_str });
        defer a.free(url);

        var client = http.HttpClient.init(a);
        defer client.deinit();

        var resp = try client.get(url);
        const body = resp.body;
        resp.body = &.{};
        return body;
    }
};
