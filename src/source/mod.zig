const std = @import("std");
const types = @import("../types.zig");
const WorkspaceSource = @import("workspace.zig").WorkspaceSource;
const LocalSource = @import("local.zig").LocalSource;
const GitSource = @import("git.zig").GitSource;
const GithubSource = @import("github.zig").GithubSource;
const RegistrySource = @import("registry.zig").RegistrySource;

pub const Source = union(enum) {
    workspace: WorkspaceSource,
    local: LocalSource,
    git: GitSource,
    github: GithubSource,
    registry: RegistrySource,
    npm: RegistrySource,

    pub fn resolve(self: Source, allocator: std.mem.Allocator, name: []const u8) ![]const u8 {
        return switch (self) {
            .workspace => |s| s.resolve(allocator, name),
            .local => |s| s.resolve(allocator, name),
            .git => |s| s.resolve(allocator, name),
            .github => |s| s.resolve(allocator, name),
            .registry, .npm => |s| s.resolve(allocator, name),
        };
    }

    pub fn fetch(self: Source, allocator: std.mem.Allocator, identity: types.PackageIdentity) ![]u8 {
        return switch (self) {
            .workspace => |s| s.fetch(allocator, identity),
            .local => |s| s.fetch(allocator, identity),
            .git => |s| s.fetch(allocator, identity),
            .github => |s| s.fetch(allocator, identity),
            .registry, .npm => |s| s.fetch(allocator, identity),
        };
    }

    pub fn deinit(self: *Source) void {
        switch (self.*) {
            .workspace => |*s| s.deinit(),
            .local => |*s| s.deinit(),
            .git => |*s| s.deinit(),
            .github => |*s| s.deinit(),
            .registry, .npm => |*s| s.deinit(),
        }
    }
};

pub const SourceError = error{
    PackageNotFound,
    VersionNotFound,
    FetchFailed,
    InvalidSource,
    GitError,
    NetworkError,
    IntegrityMismatch,
};

pub const ResolveResult = struct {
    name: []const u8,
    version: []const u8,
    package_hash: []const u8,
};

test "source: union deinit does not leak" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    try tmp.dir.writeFile(.{ .sub_path = "ara.toml", .data = "[project]\nname = \"p\"\nversion = \"0.1.0\"\n" });

    const path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(path);

    var src = Source{ .local = try LocalSource.init(std.testing.allocator, path) };
    src.deinit();
}

test "source: local fetch via union" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    try tmp.dir.writeFile(.{ .sub_path = "ara.toml", .data = "[project]\nname = \"p\"\nversion = \"0.1.0\"\n" });

    const path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(path);

    var src = Source{ .local = try LocalSource.init(std.testing.allocator, path) };
    defer src.deinit();

    const data = try src.fetch(std.testing.allocator, .{
        .source = .local,
        .name = "p",
        .version = try types.Version.parse("0.1.0"),
    });
    defer std.testing.allocator.free(data);
    try std.testing.expect(data.len > 64);
    try std.testing.expectEqual(@as(u8, 0x1f), data[0]);
    try std.testing.expectEqual(@as(u8, 0x8b), data[1]);
}
