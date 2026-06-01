const std = @import("std");
const types = @import("../types.zig");

pub const GithubSource = struct {
    allocator: std.mem.Allocator,
    repo: []const u8,

    pub fn init(allocator: std.mem.Allocator, repo: []const u8) !GithubSource {
        return .{ .allocator = allocator, .repo = try allocator.dupe(u8, repo) };
    }

    pub fn deinit(self: *GithubSource) void {
        self.allocator.free(self.repo);
    }

    pub fn resolve(self: GithubSource, a: std.mem.Allocator, n: []const u8) ![]const u8 {
        _ = a;
        _ = n;
        return self.repo;
    }

    pub fn fetch(self: GithubSource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        _ = self;
        _ = a;
        _ = id;
        return "";
    }
};
