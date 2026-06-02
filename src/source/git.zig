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

    pub fn resolve(self: GitSource, a: std.mem.Allocator, _: []const u8) ![]const u8 {
        _ = a;
        return self.commit;
    }

    pub fn fetch(self: GitSource, a: std.mem.Allocator, _: types.PackageIdentity) ![]u8 {
        const tmp_path = try std.fmt.allocPrint(a, "/tmp/ara-git-{d}", .{std.time.milliTimestamp()});
        defer a.free(tmp_path);
        defer std.fs.deleteTreeAbsolute(tmp_path) catch {};

        {
            const clone = std.process.Child.init(&.{ "git", "clone", "--depth", "1", self.url, tmp_path }, a);
            const term = try clone.spawnAndWait();
            switch (term) {
                .Exited => |code| if (code != 0) return error.GitError,
                else => return error.GitError,
            }
        }

        var archive = std.process.Child.init(&.{ "tar", "-C", tmp_path, "-cf", "-", "." }, a);
        archive.stdout_behavior = .Pipe;
        try archive.spawn();

        var buf = std.ArrayList(u8).init(a);
        errdefer buf.deinit();

        try archive.stdout.?.reader().readAllArrayList(&buf, std.math.maxInt(usize));
        _ = try archive.wait();

        return buf.toOwnedSlice();
    }
};
