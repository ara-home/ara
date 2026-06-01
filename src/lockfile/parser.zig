const std = @import("std");
const toml = @import("../util/toml.zig");
const Lockfile = @import("types.zig").Lockfile;
const PackageEntry = @import("types.zig").PackageEntry;
const GraphMeta = @import("types.zig").GraphMeta;
const SecurityMeta = @import("types.zig").SecurityMeta;
const SbomMeta = @import("types.zig").SbomMeta;

pub const Error = error{
    InvalidLockfileVersion,
    MissingPackageHash,
};

pub fn parse(allocator: std.mem.Allocator, content: []const u8) !Lockfile {
    var doc = try toml.parse(allocator, content);
    defer doc.deinit(allocator);

    var lockfile = Lockfile{};

    if (doc.getEntry(null, "version")) |v| {
        _ = v;
    }

    if (doc.getTable("graph")) |tbl| {
        var meta = GraphMeta{};
        for (tbl.entries.items) |e| {
            if (std.mem.eql(u8, e.key, "resolver")) {
                meta.resolver = e.value.asString() orelse "mvs";
            } else if (std.mem.eql(u8, e.key, "generated_at")) {
                meta.generated_at = e.value.asString();
            } else if (std.mem.eql(u8, e.key, "graph_hash")) {
                meta.graph_hash = e.value.asString();
            }
        }
        lockfile.graph = meta;
    }

    if (doc.getArrays("package")) |arr| {
        var packages = std.ArrayList(PackageEntry).init(allocator);
        for (arr) |pkg_table| {
            const entry = try parsePackageEntry(&pkg_table);
            try packages.append(entry);
        }
        lockfile.packages = try packages.toOwnedSlice();
    }

    return lockfile;
}

fn parsePackageEntry(table: *const toml.ArrayOfTables) !PackageEntry {
    var entry = PackageEntry{
        .name = "",
        .version = "",
        .source = "",
        .package_hash = "",
    };

    for (table.entries.items) |e| {
        if (std.mem.eql(u8, e.key, "name")) {
            entry.name = e.value.asString() orelse "";
        } else if (std.mem.eql(u8, e.key, "version")) {
            entry.version = e.value.asString() orelse "";
        } else if (std.mem.eql(u8, e.key, "source")) {
            entry.source = e.value.asString() orelse "";
        } else if (std.mem.eql(u8, e.key, "package_hash")) {
            entry.package_hash = e.value.asString() orelse return error.MissingPackageHash;
        } else if (std.mem.eql(u8, e.key, "integrity")) {
            entry.integrity = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "signature")) {
            entry.signature = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "repository")) {
            entry.repository = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "commit")) {
            entry.commit = e.value.asString();
        } else if (std.mem.eql(u8, e.key, "dependencies")) {
            const arr = switch (e.value) {
                .array => |items| items,
                else => continue,
            };
            _ = arr;
        }
    }

    return entry;
}

test "lockfile: parse minimal" {
    const src =
        \\version = 1
        \\
        \\[graph]
        \\resolver = "mvs"
        \\graph_hash = "sha256:abc"
        \\
        \\[[package]]
        \\name = "zod"
        \\version = "3.23.8"
        \\source = "npm"
        \\package_hash = "sha256:def"
    ;
    var lf = try parse(std.testing.allocator, src);
    defer lf.deinit(std.testing.allocator);

    try std.testing.expectEqual(@as(u32, 1), lf.version);
    try std.testing.expectEqualStrings("mvs", lf.graph.resolver);
    try std.testing.expectEqualStrings("sha256:abc", lf.graph.graph_hash.?);
    try std.testing.expectEqual(@as(usize, 1), lf.packages.len);
    try std.testing.expectEqualStrings("zod", lf.packages[0].name);
    try std.testing.expectEqualStrings("npm", lf.packages[0].source);
}

test "lockfile: parse with graph meta" {
    const src =
        \\version = 1
        \\
        \\[graph]
        \\resolver = "mvs"
        \\generated_at = "2026-06-01T22:00:00Z"
        \\graph_hash = "sha256:789"
        \\
        \\[[package]]
        \\name = "react"
        \\version = "18.3.0"
        \\source = "github"
        \\package_hash = "sha256:xyz"
    ;
    var lf = try parse(std.testing.allocator, src);
    defer lf.deinit(std.testing.allocator);

    try std.testing.expectEqualStrings("mvs", lf.graph.resolver);
    try std.testing.expectEqualStrings("2026-06-01T22:00:00Z", lf.graph.generated_at.?);
    try std.testing.expectEqualStrings("sha256:789", lf.graph.graph_hash.?);
    try std.testing.expectEqual(@as(usize, 1), lf.packages.len);
    try std.testing.expectEqualStrings("react", lf.packages[0].name);
}
