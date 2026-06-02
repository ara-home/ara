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

    pub fn resolve(self: LocalSource, a: std.mem.Allocator, _: []const u8) ![]const u8 {
        _ = a;
        return self.path;
    }

    pub fn fetch(self: LocalSource, a: std.mem.Allocator, _: types.PackageIdentity) ![]u8 {
        const manifest_path = try std.fs.path.join(a, &.{ self.path, "ara.toml" });
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
