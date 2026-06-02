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
        var child = std.process.Child.init(&.{ "tar", "-C", self.root_path, "-czf", "-", "." }, a);
        child.stdout_behavior = .Pipe;
        try child.spawn();

        var buf = std.ArrayList(u8).init(a);
        errdefer buf.deinit();
        try child.stdout.?.reader().readAllArrayList(&buf, std.math.maxInt(usize));

        const term = try child.wait();
        switch (term) {
            .Exited => |code| if (code != 0) return error.TarFailed,
            else => return error.TarFailed,
        }

        return buf.toOwnedSlice();
    }
};

test "workspace: fetch produces valid tar" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();

    try tmp.dir.writeFile(.{ .sub_path = "ara.toml", .data = "[project]\nname = \"ws-member\"\nversion = \"0.1.0\"\n" });

    const path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(path);

    var src = try WorkspaceSource.init(std.testing.allocator, path);
    defer src.deinit();

    const id = types.PackageIdentity{
        .source = .workspace,
        .name = "ws-member",
        .version = try types.Version.parse("0.1.0"),
    };
    const tarball = try src.fetch(std.testing.allocator, id);
    defer std.testing.allocator.free(tarball);

    try std.testing.expect(tarball.len > 64);
    try std.testing.expectEqual(@as(u8, 0x1f), tarball[0]); // gzip magic
    try std.testing.expectEqual(@as(u8, 0x8b), tarball[1]);
}
