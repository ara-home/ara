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
        var child = std.process.Child.init(&.{ "tar", "-C", self.root_path, "-cf", "-", "." }, a);
        child.stdout_behavior = .Pipe;
        try child.spawn();

        var buf = std.ArrayList(u8).init(a);
        errdefer buf.deinit();
        try child.stdout.?.reader().readAllArrayList(&buf, std.math.maxInt(usize));
        _ = try child.wait();

        return buf.toOwnedSlice();
    }
};
