const std = @import("std");
const types = @import("../types.zig");

pub const GitSource = struct {
    allocator: std.mem.Allocator,
    url: []const u8,
    commit: []const u8,

    pub fn init(allocator: std.mem.Allocator, url: []const u8, commit: []const u8) !GitSource {
        return .{
            .allocator = allocator,
            .url = try allocator.dupe(u8, url),
            .commit = try allocator.dupe(u8, commit),
        };
    }

    pub fn deinit(self: *GitSource) void {
        self.allocator.free(self.url);
        self.allocator.free(self.commit);
    }

    pub fn resolve(self: GitSource, a: std.mem.Allocator, n: []const u8) ![]const u8 {
        _ = a;
        _ = n;
        return self.commit;
    }

    pub fn fetch(self: GitSource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        _ = self;
        _ = a;
        _ = id;
        return "";
    }
};
