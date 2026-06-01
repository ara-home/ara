const std = @import("std");

pub const PackageEntry = struct {
    name: []const u8,
    version: []const u8,
    source: []const u8,
    integrity: ?[]const u8 = null,
    package_hash: []const u8,
    signature: ?[]const u8 = null,
    repository: ?[]const u8 = null,
    commit: ?[]const u8 = null,
    dependencies: ?[]const []const u8 = null,
};

pub const Lockfile = struct {
    version: u32 = 1,
    graph: Graph = .{},
    packages: ?[]PackageEntry = null,
};

pub const Graph = struct {
    resolver: []const u8 = "mvs",
    generated_at: ?[]const u8 = null,
    graph_hash: ?[]const u8 = null,
};
