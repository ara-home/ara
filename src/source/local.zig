const std = @import("std");
const types = @import("../types.zig");

pub const LocalSource = struct {
    allocator: std.mem.Allocator,
    path: []const u8,

    pub fn init(allocator: std.mem.Allocator, path: []const u8) !LocalSource {
        return .{ .allocator = allocator, .path = try allocator.dupe(u8, path) };
    }

    pub fn deinit(self: *LocalSource) void {
        self.allocator.free(self.path);
    }

    pub fn resolve(self: LocalSource, a: std.mem.Allocator, n: []const u8) ![]const u8 {
        _ = a;
        _ = n;
        return self.path;
    }

    pub fn fetch(self: LocalSource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        _ = self;
        _ = a;
        _ = id;
        return "";
    }
};
