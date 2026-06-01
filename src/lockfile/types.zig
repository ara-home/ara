const std = @import("std");

pub const GraphMeta = struct {
    resolver: []const u8 = "mvs",
    generated_at: ?[]const u8 = null,
    graph_hash: ?[]const u8 = null,
};

pub const SecurityMeta = struct {
    risk_level: ?[]const u8 = null,
    analysis_version: ?[]const u8 = null,
};

pub const SbomMeta = struct {
    license: ?[]const u8 = null,
    supplier: ?[]const u8 = null,
};

pub const PackageEntry = struct {
    name: []const u8,
    version: []const u8,
    source: []const u8,
    package_hash: []const u8,
    integrity: ?[]const u8 = null,
    signature: ?[]const u8 = null,
    repository: ?[]const u8 = null,
    commit: ?[]const u8 = null,
    dependencies: ?[]const []const u8 = null,
    security: ?SecurityMeta = null,
    sbom: ?SbomMeta = null,
};

pub const Lockfile = struct {
    version: u32 = 1,
    graph: GraphMeta = .{},
    packages: []const PackageEntry = &.{},

    pub fn deinit(self: *Lockfile, allocator: std.mem.Allocator) void {
        if (self.packages.len > 0) {
            const slice: []const PackageEntry = self.packages;
            allocator.free(std.mem.sliceAsBytes(slice));
        }
    }
};
