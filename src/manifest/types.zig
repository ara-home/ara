const std = @import("std");
const types = @import("../types.zig");

pub const Project = struct {
    name: []const u8,
    version: []const u8,
    description: ?[]const u8 = null,
    license: ?[]const u8 = null,
    repository: ?[]const u8 = null,
    homepage: ?[]const u8 = null,
};

pub const DependencyEntry = struct {
    name: []const u8,
    source: types.SourceType,
    version: ?[]const u8 = null,
    repo: ?[]const u8 = null,
    url: ?[]const u8 = null,
    commit: ?[]const u8 = null,
    path: ?[]const u8 = null,
    package: ?[]const u8 = null,
};

pub const Workspace = struct {
    members: []const []const u8,
};

pub const ScriptEntry = struct { name: []const u8, command: []const u8 };

pub const Scripts = struct {
    entries: []const ScriptEntry,
};

pub const Security = struct {
    risk_threshold: ?types.RiskLevel = null,
    require_review: ?bool = null,
    allow_lifecycle_scripts: ?bool = null,
    block_critical: ?bool = null,
};

pub const Build = struct {
    hermetic: ?bool = null,
    offline_first: ?bool = null,
};

pub const Manifest = struct {
    project: Project = .{ .name = "", .version = "" },
    deps: []DependencyEntry = &.{},
    workspace: ?Workspace = null,
    scripts: ?Scripts = null,
    security: ?Security = null,
    build: ?Build = null,

    pub fn deinit(self: *Manifest, allocator: std.mem.Allocator) void {
        if (self.deps.len > 0) allocator.free(std.mem.sliceAsBytes(self.deps));
        if (self.workspace) |*ws| {
            if (ws.members.len > 0) allocator.free(std.mem.sliceAsBytes(ws.members));
        }
        if (self.scripts) |*s| {
            if (s.entries.len > 0) allocator.free(std.mem.sliceAsBytes(s.entries));
        }
    }
};
