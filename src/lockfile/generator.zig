const std = @import("std");
const Lockfile = @import("types.zig").Lockfile;

pub fn generate(allocator: std.mem.Allocator, lockfile: Lockfile) ![]u8 {
    var buf = std.ArrayList(u8).init(allocator);
    errdefer buf.deinit();

    try std.fmt.format(buf.writer(), "version = {d}\n\n", .{lockfile.version});
    try std.fmt.format(buf.writer(), "[graph]\n", .{});
    try std.fmt.format(buf.writer(), "resolver = \"{s}\"\n", .{lockfile.graph.resolver});
    if (lockfile.graph.generated_at) |t| try std.fmt.format(buf.writer(), "generated_at = \"{s}\"\n", .{t});
    if (lockfile.graph.graph_hash) |h| try std.fmt.format(buf.writer(), "graph_hash = \"{s}\"\n", .{h});
    try std.fmt.format(buf.writer(), "\n", .{});

    for (lockfile.packages) |pkg| {
        try std.fmt.format(buf.writer(), "[[package]]\n", .{});
        try std.fmt.format(buf.writer(), "name = \"{s}\"\n", .{pkg.name});
        try std.fmt.format(buf.writer(), "version = \"{s}\"\n", .{pkg.version});
        try std.fmt.format(buf.writer(), "source = \"{s}\"\n", .{pkg.source});
        if (pkg.integrity) |v| try std.fmt.format(buf.writer(), "integrity = \"{s}\"\n", .{v});
        try std.fmt.format(buf.writer(), "package_hash = \"{s}\"\n", .{pkg.package_hash});
        if (pkg.signature) |v| try std.fmt.format(buf.writer(), "signature = \"{s}\"\n", .{v});
        if (pkg.repository) |v| try std.fmt.format(buf.writer(), "repository = \"{s}\"\n", .{v});
        if (pkg.commit) |v| try std.fmt.format(buf.writer(), "commit = \"{s}\"\n", .{v});
        if (pkg.dependencies) |deps| {
            try std.fmt.format(buf.writer(), "dependencies = [", .{});
            for (deps, 0..) |dep, i| {
                if (i > 0) try std.fmt.format(buf.writer(), ", ", .{});
                try std.fmt.format(buf.writer(), "\"{s}\"", .{dep});
            }
            try std.fmt.format(buf.writer(), "]\n", .{});
        }
        try std.fmt.format(buf.writer(), "\n", .{});
    }

    return buf.toOwnedSlice();
}

test "lockfile: generate and parse back" {
    const lf = Lockfile{
        .graph = .{
            .resolver = "mvs",
            .generated_at = "2026-06-01T22:00:00Z",
            .graph_hash = "sha256:abc",
        },
        .packages = &.{
            .{
                .name = "zod",
                .version = "3.23.8",
                .source = "npm",
                .package_hash = "sha256:def",
            },
        },
    };

    const output = try generate(std.testing.allocator, lf);
    defer std.testing.allocator.free(output);

    try std.testing.expect(std.mem.containsAtLeast(u8, output, 1, "zod"));
    try std.testing.expect(std.mem.containsAtLeast(u8, output, 1, "3.23.8"));
    try std.testing.expect(std.mem.containsAtLeast(u8, output, 1, "mvs"));
}
