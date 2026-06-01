const std = @import("std");
const types = @import("../types.zig");
const toml = @import("../util/toml.zig");
const Manifest = @import("types.zig").Manifest;
const Project = @import("types.zig").Project;
const DependencyEntry = @import("types.zig").DependencyEntry;
const Workspace = @import("types.zig").Workspace;
const Scripts = @import("types.zig").Scripts;
const ScriptEntry = @import("types.zig").ScriptEntry;
const Security = @import("types.zig").Security;
const Build = @import("types.zig").Build;

pub const Error = error{
    MissingProjectName,
    MissingProjectVersion,
    UnknownSourceType,
    InvalidRiskLevel,
};

pub fn parse(allocator: std.mem.Allocator, content: []const u8) !Manifest {
    var doc = try toml.parse(allocator, content);
    defer doc.deinit(allocator);

    var manifest = Manifest{};

    if (doc.getTable("project")) |tbl| {
        manifest.project = try parseProject(tbl);
    }

    if (doc.getTable("deps")) |tbl| {
        const deps = try parseDeps(allocator, tbl);
        manifest.deps = deps;
    }

    if (doc.getTable("workspace")) |tbl| {
        manifest.workspace = try parseWorkspace(allocator, tbl);
    }

    if (doc.getTable("scripts")) |tbl| {
        manifest.scripts = try parseScripts(allocator, tbl);
    }

    if (doc.getTable("security")) |tbl| {
        manifest.security = try parseSecurity(tbl);
    }

    if (doc.getTable("build")) |tbl| {
        manifest.build = try parseBuild(tbl);
    }

    return manifest;
}

fn parseProject(table: *const toml.Table) !Project {
    var proj = Project{ .name = "", .version = "" };

    for (table.entries.items) |e| {
        if (std.mem.eql(u8, e.key, "name")) {
            proj.name = e.value.asString() orelse return error.MissingProjectName;
        } else if (std.mem.eql(u8, e.key, "version")) {
            proj.version = e.value.asString() orelse return error.MissingProjectVersion;
        } else if (std.mem.eql(u8, e.key, "description")) {
            proj.description = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "license")) {
            proj.license = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "repository")) {
            proj.repository = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "homepage")) {
            proj.homepage = e.value.asString();
        }
    }

    return proj;
}

fn parseDeps(allocator: std.mem.Allocator, table: *const toml.Table) ![]DependencyEntry {
    var deps = std.ArrayList(DependencyEntry).init(allocator);

    for (table.entries.items) |e| {
        const src_type_str = e.value.getInline("source") orelse return error.UnknownSourceType;
        const src_str = src_type_str.asString() orelse return error.UnknownSourceType;
        const source = types.SourceType.fromString(src_str) catch return error.UnknownSourceType;

        const entry = DependencyEntry{
            .name = e.key,
            .source = source,
            .version = if (e.value.getInline("version")) |v| v.asString() else null,
            .repo = if (e.value.getInline("repo")) |v| v.asString() else null,
            .url = if (e.value.getInline("url")) |v| v.asString() else null,
            .commit = if (e.value.getInline("commit")) |v| v.asString() else null,
            .path = if (e.value.getInline("path")) |v| v.asString() else null,
            .package = if (e.value.getInline("package")) |v| v.asString() else null,
        };

        try deps.append(entry);
    }

    return deps.toOwnedSlice();
}

fn parseWorkspace(allocator: std.mem.Allocator, table: *const toml.Table) !Workspace {
    for (table.entries.items) |e| {
        if (std.mem.eql(u8, e.key, "members")) {
            const arr = switch (e.value) {
                .array => |items| items,
                else => return error.InvalidManifest,
            };
            const member_list = try allocator.alloc([]const u8, arr.len);
            for (arr, 0..) |v, i| {
                member_list[i] = switch (v) {
                    .string => |s| s,
                    else => return error.InvalidManifest,
                };
            }
            return Workspace{ .members = member_list };
        }
    }
    return Workspace{ .members = &.{} };
}

fn parseScripts(allocator: std.mem.Allocator, table: *const toml.Table) !Scripts {
    var script_list = std.ArrayList(ScriptEntry).init(allocator);

    for (table.entries.items) |e| {
        const cmd = e.value.asString() orelse continue;
        try script_list.append(.{ .name = e.key, .command = cmd });
    }

    return Scripts{ .entries = try script_list.toOwnedSlice() };
}

fn parseSecurity(table: *const toml.Table) !Security {
    var sec = Security{};

    for (table.entries.items) |e| {
        if (std.mem.eql(u8, e.key, "risk_threshold")) {
            const val = e.value.asString() orelse continue;
            sec.risk_threshold = std.meta.stringToEnum(types.RiskLevel, val);
        } else if (std.mem.eql(u8, e.key, "require_review")) {
            sec.require_review = e.value.asBoolean();
        } else if (std.mem.eql(u8, e.key, "allow_lifecycle_scripts")) {
            sec.allow_lifecycle_scripts = e.value.asBoolean();
        } else if (std.mem.eql(u8, e.key, "block_critical")) {
            sec.block_critical = e.value.asBoolean();
        }
    }

    return sec;
}

fn parseBuild(table: *const toml.Table) !Build {
    var b = Build{};

    for (table.entries.items) |e| {
        if (std.mem.eql(u8, e.key, "hermetic")) {
            b.hermetic = e.value.asBoolean();
        } else if (std.mem.eql(u8, e.key, "offline_first")) {
            b.offline_first = e.value.asBoolean();
        }
    }

    return b;
}

test "manifest: parse minimal" {
    const src =
        \\[project]
        \\name = "my-app"
        \\version = "0.1.0"
    ;
    var m = try parse(std.testing.allocator, src);
    defer m.deinit(std.testing.allocator);
    try std.testing.expectEqualStrings("my-app", m.project.name);
    try std.testing.expectEqualStrings("0.1.0", m.project.version);
}

test "manifest: parse with deps" {
    const src =
        \\[project]
        \\name = "app"
        \\version = "1.0.0"
        \\
        \\[deps]
        \\zod = { source = "npm", version = "3.23.8" }
        \\react = { source = "github", repo = "facebook/react", version = "18.x" }
    ;
    var m = try parse(std.testing.allocator, src);
    defer m.deinit(std.testing.allocator);
    try std.testing.expectEqual(@as(usize, 2), m.deps.len);
    try std.testing.expectEqualStrings("zod", m.deps[0].name);
    try std.testing.expectEqual(.npm, m.deps[0].source);
    try std.testing.expectEqualStrings("3.23.8", m.deps[0].version.?);
    try std.testing.expectEqualStrings("react", m.deps[1].name);
    try std.testing.expectEqualStrings("facebook/react", m.deps[1].repo.?);
}

test "manifest: parse workspace" {
    const src =
        \\[project]
        \\name = "monorepo"
        \\version = "0.1.0"
        \\
        \\[workspace]
        \\members = ["apps/*", "packages/*"]
    ;
    var m = try parse(std.testing.allocator, src);
    defer m.deinit(std.testing.allocator);
    try std.testing.expect(m.workspace != null);
    try std.testing.expectEqual(@as(usize, 2), m.workspace.?.members.len);
    try std.testing.expectEqualStrings("apps/*", m.workspace.?.members[0]);
}

test "manifest: parse security and build" {
    const src =
        \\[project]
        \\name = "secure-app"
        \\version = "0.1.0"
        \\
        \\[security]
        \\risk_threshold = "medium"
        \\require_review = true
        \\
        \\[build]
        \\hermetic = true
        \\offline_first = true
    ;
    var m = try parse(std.testing.allocator, src);
    defer m.deinit(std.testing.allocator);
    try std.testing.expectEqual(.medium, m.security.?.risk_threshold.?);
    try std.testing.expectEqual(true, m.security.?.require_review.?);
    try std.testing.expectEqual(true, m.build.?.hermetic.?);
    try std.testing.expectEqual(true, m.build.?.offline_first.?);
}
