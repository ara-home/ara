const std = @import("std");
const types = @import("../types.zig");

pub const Project = struct {
    name: []const u8,
    version: []const u8,
    description: ?[]const u8 = null,
    license: ?[]const u8 = null,
    repository: ?[]const u8 = null,
    homepage: ?[]const u8 = null,
    authors: ?[]const []const u8 = null,
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
    project: ?Project = null,
    deps: ?[]DependencyEntry = null,
    workspace: ?Workspace = null,
    scripts: ?std.StringHashMap([]const u8) = null,
    security: ?Security = null,
    build: ?Build = null,
};

pub const Workspace = struct {
    members: []const []const u8,
};
