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
        var child = std.process.Child.init(&.{ "tar", "-C", self.path, "-cf", "-", "." }, a);
        child.stdout_behavior = .Pipe;
        try child.spawn();

        var buf = std.ArrayList(u8).init(a);
        errdefer buf.deinit();
        try child.stdout.?.reader().readAllArrayList(&buf, std.math.maxInt(usize));
        _ = try child.wait();

        return buf.toOwnedSlice();
    }
};
