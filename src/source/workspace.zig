const std = @import("std");
const types = @import("../types.zig");

pub const WorkspaceSource = struct {
    allocator: std.mem.Allocator,
    root_path: []const u8,

    pub fn init(allocator: std.mem.Allocator, root_path: []const u8) !WorkspaceSource {
        return .{
            .allocator = allocator,
            .root_path = try allocator.dupe(u8, root_path),
        };
    }

    pub fn deinit(self: *WorkspaceSource) void {
        self.allocator.free(self.root_path);
    }

    pub fn resolve(self: WorkspaceSource, a: std.mem.Allocator, n: []const u8) ![]const u8 {
        _ = a;
        _ = n;
        return self.root_path;
    }

    pub fn fetch(self: WorkspaceSource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        _ = self;
        _ = a;
        _ = id;
        return "";
    }
};
