const std = @import("std");
const types = @import("../types.zig");
const manifest = @import("../manifest/parser.zig");
const lockfile_parser = @import("../lockfile/parser.zig");
const lockfile_types = @import("../lockfile/types.zig");
const lockfile_gen = @import("../lockfile/generator.zig");
const resolver = @import("../resolver/mvs.zig");
const store_cas = @import("../store/cas.zig");

pub const Command = enum {
    install,
    run,
    build,
    publish,
    gc,
    trust,
    unknown,

    pub fn fromString(s: []const u8) Command {
        return std.meta.stringToEnum(Command, s) orelse .unknown;
    }
};

pub fn install(allocator: std.mem.Allocator, cwd: []const u8) !void {
    const manifest_path = try std.fs.path.join(allocator, &.{ cwd, "ara.toml" });
    defer allocator.free(manifest_path);

    const file = std.fs.openFileAbsolute(manifest_path, .{ .mode = .read_only }) catch |err| switch (err) {
        error.FileNotFound => {
            std.debug.print("error: ara.toml not found in {s}\n", .{cwd});
            return err;
        },
        else => return err,
    };
    defer file.close();

    const stat = try file.stat();
    const content = try allocator.alloc(u8, @intCast(stat.size));
    defer allocator.free(content);
    _ = try file.readAll(content);

    const m = try manifest.parse(allocator, content);
    defer m.deinit(allocator);

    std.debug.print("Installing dependencies for {s} v{s}\n", .{ m.project.name, m.project.version });

    if (m.deps.len == 0) {
        std.debug.print("No dependencies to install\n", .{});
        return;
    }

    var resolve = resolver.Resolver.init(allocator);
    defer resolve.deinit();

    for (m.deps) |dep| {
        const constraint = try types.Constraint.parse(dep.version orelse "*");
        try resolve.addConstraint(.{
            .package = dep.name,
            .constraint = constraint,
            .source = dep.source,
            .required_by = "root",
        });
    }

    var graph = try resolve.resolve();
    defer graph.deinit();

    std.debug.print("Resolved {d} packages\n", .{graph.nodes.items.len});

    const home = std.process.getenv("HOME") orelse ".";
    const store_base = try std.fs.path.join(allocator, &.{ home, ".ara", "store" });
    defer allocator.free(store_base);

    var store = try store_cas.Store.init(allocator, store_base);
    defer store.deinit();
    try store.ensureDirs();

    for (graph.nodes.items) |node| {
        var ver_buf: [64]u8 = undefined;
        const ver_str = try std.fmt.bufPrint(&ver_buf, "{}.{}.{}", .{ node.version.major, node.version.minor, node.version.patch });
        std.debug.print("  {s}@{s}\n", .{ node.name, ver_str });
    }

    const lock_content = try lockfile_gen.generate(allocator, .{
        .graph = .{
            .resolver = "mvs",
            .generated_at = "TODO",
            .graph_hash = null,
        },
        .packages = &.{},
    });
    defer allocator.free(lock_content);

    const lock_path = try std.fs.path.join(allocator, &.{ cwd, "ara.lock" });
    defer allocator.free(lock_path);

    const lock_file = try std.fs.createFileAbsolute(lock_path, .{});
    defer lock_file.close();
    try lock_file.writeAll(lock_content);

    std.debug.print("Lockfile written to ara.lock\n", .{});
}

pub fn run(allocator: std.mem.Allocator, _: []const u8, script: []const u8) !void {
    std.debug.print("running: {s}\n", .{script});

    const child = std.process.Child.init(&.{ "sh", "-c", script }, allocator);
    _ = try child.spawnAndWait();
}
