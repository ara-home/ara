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
            var clone = std.process.Child.init(&.{ "git", "clone", "--depth", "1", self.url, tmp_path }, a);
            const term = try clone.spawnAndWait();
            switch (term) {
                .Exited => |code| if (code != 0) return error.GitError,
                else => return error.GitError,
            }
        }

        var archive = std.process.Child.init(&.{ "tar", "-C", tmp_path, "-czf", "-", "." }, a);
        archive.stdout_behavior = .Pipe;
        try archive.spawn();

        var buf = std.ArrayList(u8).init(a);
        errdefer buf.deinit();

        try archive.stdout.?.reader().readAllArrayList(&buf, std.math.maxInt(usize));

        const term = try archive.wait();
        switch (term) {
            .Exited => |code| if (code != 0) return error.GitError,
            else => return error.GitError,
        }

        return buf.toOwnedSlice();
    }
};

fn haveBinary(name: []const u8) bool {
    const result = std.process.Child.run(.{ .allocator = std.testing.allocator, .argv = &.{ name, "--version" } }) catch return false;
    defer {
        std.testing.allocator.free(result.stdout);
        std.testing.allocator.free(result.stderr);
    }
    return result.term.Exited == 0;
}

test "git: fetch from local repo" {
    if (!haveBinary("git")) return error.Skip;
    if (!haveBinary("tar")) return error.Skip;

    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();

    const path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(path);

    try tmp.dir.writeFile(.{ .sub_path = "hello.txt", .data = "world" });

    {
        const argv = [_][]const u8{ "git", "-C", path, "init" };
        var child = std.process.Child.init(&argv, std.testing.allocator);
        child.stderr_behavior = .Ignore;
        const term = try child.spawnAndWait();
        try std.testing.expectEqual(std.process.Child.Term{ .Exited = 0 }, term);
    }
    {
        const argv = [_][]const u8{ "git", "-C", path, "config", "user.email", "test@test.com" };
        var child = std.process.Child.init(&argv, std.testing.allocator);
        child.stderr_behavior = .Ignore;
        const term = try child.spawnAndWait();
        try std.testing.expectEqual(std.process.Child.Term{ .Exited = 0 }, term);
    }
    {
        const argv = [_][]const u8{ "git", "-C", path, "config", "user.name", "test" };
        var child = std.process.Child.init(&argv, std.testing.allocator);
        child.stderr_behavior = .Ignore;
        const term = try child.spawnAndWait();
        try std.testing.expectEqual(std.process.Child.Term{ .Exited = 0 }, term);
    }
    {
        const argv = [_][]const u8{ "git", "-C", path, "config", "commit.gpgsign", "false" };
        var child = std.process.Child.init(&argv, std.testing.allocator);
        child.stderr_behavior = .Ignore;
        const term = try child.spawnAndWait();
        try std.testing.expectEqual(std.process.Child.Term{ .Exited = 0 }, term);
    }
    {
        const argv = [_][]const u8{ "git", "-C", path, "add", "." };
        var child = std.process.Child.init(&argv, std.testing.allocator);
        child.stderr_behavior = .Ignore;
        const term = try child.spawnAndWait();
        try std.testing.expectEqual(std.process.Child.Term{ .Exited = 0 }, term);
    }
    {
        const argv = [_][]const u8{ "git", "-C", path, "commit", "-m", "init" };
        var child = std.process.Child.init(&argv, std.testing.allocator);
        child.stderr_behavior = .Ignore;
        const term = try child.spawnAndWait();
        try std.testing.expectEqual(std.process.Child.Term{ .Exited = 0 }, term);
    }

    var src = try GitSource.init(std.testing.allocator, path, "HEAD");
    defer src.deinit();

    const id = types.PackageIdentity{
        .source = .git,
        .name = "test-repo",
        .version = try types.Version.parse("0.1.0"),
    };
    const tarball = try src.fetch(std.testing.allocator, id);
    defer std.testing.allocator.free(tarball);

    try std.testing.expect(tarball.len > 0);
}
