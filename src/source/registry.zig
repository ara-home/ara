const std = @import("std");
const types = @import("../types.zig");

pub const RegistrySource = struct {
    allocator: std.mem.Allocator,
    registry_url: []const u8,

    pub fn init(allocator: std.mem.Allocator, registry_url: []const u8) !RegistrySource {
        return .{ .allocator = allocator, .registry_url = try allocator.dupe(u8, registry_url) };
    }

    pub fn deinit(self: *RegistrySource) void {
        self.allocator.free(self.registry_url);
    }

    pub fn resolve(self: RegistrySource, a: std.mem.Allocator, n: []const u8) ![]const u8 {
        _ = self;
        _ = a;
        _ = n;
        return "";
    }

    pub fn fetch(self: RegistrySource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        _ = self;
        _ = a;
        _ = id;
        return "";
    }
};
