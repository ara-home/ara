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

    pub fn resolve(self: WorkspaceSource, a: std.mem.Allocator, _: []const u8) ![]const u8 {
        _ = a;
        return self.root_path;
    }

    pub fn fetch(self: WorkspaceSource, a: std.mem.Allocator, _: types.PackageIdentity) ![]u8 {
        const manifest_path = try std.fs.path.join(a, &.{ self.root_path, "ara.toml" });
        defer a.free(manifest_path);

        const file = std.fs.openFileAbsolute(manifest_path, .{ .mode = .read_only }) catch |err| switch (err) {
            error.FileNotFound => return error.PackageNotFound,
            else => return err,
        };
        defer file.close();

        const stat = try file.stat();
        const buf = try a.alloc(u8, @intCast(stat.size));
        errdefer a.free(buf);
        _ = try file.readAll(buf);
        return buf;
    }
};
