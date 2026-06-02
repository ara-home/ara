const std = @import("std");
const types = @import("../types.zig");
const manifest = @import("../manifest/parser.zig");
const lockfile_types = @import("../lockfile/types.zig");
const lockfile_gen = @import("../lockfile/generator.zig");
const manifest_types = @import("../manifest/types.zig");
const resolver = @import("../resolver/mvs.zig");
const store_cas = @import("../store/cas.zig");
const source_mod = @import("../source/mod.zig");
const RegistrySource = @import("../source/registry.zig").RegistrySource;
const GithubSource = @import("../source/github.zig").GithubSource;
const GitSource = @import("../source/git.zig").GitSource;
const LocalSource = @import("../source/local.zig").LocalSource;
const WorkspaceSource = @import("../source/workspace.zig").WorkspaceSource;

const DependencyEntry = manifest_types.DependencyEntry;

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

fn currentTimestamp(allocator: std.mem.Allocator) ![]u8 {
    const ts = std.time.epoch.EpochSeconds{ .secs = @intCast(@max(0, std.time.timestamp())) };
    const epoch_day = ts.getEpochDay();
    const year_day = epoch_day.calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    const day_seconds = ts.getDaySeconds();
    return std.fmt.allocPrint(allocator, "{d:0>4}-{d:0>2}-{d:0>2}T{d:0>2}:{d:0>2}:{d:0>2}Z", .{
        year_day.year,
        month_day.month.numeric() + 1,
        month_day.day_index + 1,
        day_seconds.getHoursIntoDay(),
        day_seconds.getMinutesIntoHour(),
        day_seconds.getSecondsIntoMinute(),
    });
}

fn findDep(deps: []const DependencyEntry, name: []const u8) ?DependencyEntry {
    for (deps) |d| {
        if (std.mem.eql(u8, d.name, name)) return d;
    }
    return null;
}

fn createSource(allocator: std.mem.Allocator, source_type: types.SourceType, dep: DependencyEntry) !source_mod.Source {
    return switch (source_type) {
        .npm, .registry => source_mod.Source{ .registry = try RegistrySource.init(allocator, dep.url orelse "https://registry.npmjs.org") },
        .github => source_mod.Source{ .github = try GithubSource.init(allocator, dep.repo orelse return error.MissingRepo) },
        .git => source_mod.Source{ .git = try GitSource.init(allocator, dep.url orelse return error.MissingUrl, dep.commit orelse "HEAD") },
        .local => source_mod.Source{ .local = try LocalSource.init(allocator, dep.path orelse return error.MissingPath) },
        .workspace => source_mod.Source{ .workspace = try WorkspaceSource.init(allocator, dep.path orelse ".") },
    };
}

fn extractTarball(allocator: std.mem.Allocator, tarball: []const u8, dest: []const u8) !void {
    var child = std.process.Child.init(&.{ "tar", "-xzf", "-", "-C", dest }, allocator);
    child.stdin_behavior = .Pipe;
    try child.spawn();
    try child.stdin.?.writeAll(tarball);
    child.stdin.?.close();
    child.stdin = null;
    const term = try child.wait();
    switch (term) {
        .Exited => |code| if (code != 0) return error.ExtractFailed,
        else => return error.ExtractFailed,
    }
}

test "currentTimestamp format" {
    const ts = try currentTimestamp(std.testing.allocator);
    defer std.testing.allocator.free(ts);
    try std.testing.expect(ts.len == 20);
    try std.testing.expect(ts[4] == '-');
    try std.testing.expect(ts[7] == '-');
    try std.testing.expect(ts[10] == 'T');
    try std.testing.expect(ts[19] == 'Z');
}

test "findDep finds matching dependency" {
    const deps = [_]DependencyEntry{
        .{ .name = "zod", .source = .npm },
        .{ .name = "react", .source = .github },
    };
    const found = findDep(&deps, "zod");
    try std.testing.expect(found != null);
    try std.testing.expectEqualStrings("zod", found.?.name);
    try std.testing.expectEqual(.npm, found.?.source);
}

test "findDep returns null for missing" {
    const deps = [_]DependencyEntry{
        .{ .name = "zod", .source = .npm },
    };
    try std.testing.expect(findDep(&deps, "missing") == null);
}

pub fn install(allocator: std.mem.Allocator, cwd: []const u8) !void {
    const manifest_path = try std.fs.path.join(allocator, &.{ cwd, "ara.toml" });
    defer allocator.free(manifest_path);

    const file = std.fs.openFileAbsolute(manifest_path, .{ .mode = .read_only }) catch |err| switch (err) {
        error.FileNotFound => {
            std.debug.print("error: ara.toml not found\n", .{});
            return err;
        },
        else => return err,
    };
    defer file.close();

    const stat = try file.stat();
    const content = try allocator.alloc(u8, @intCast(stat.size));
    defer allocator.free(content);
    _ = try file.readAll(content);

    var m = try manifest.parse(allocator, content);
    defer m.deinit(allocator);

    std.debug.print("Installing dependencies for {s} v{s}\n", .{ m.project.name, m.project.version });

    if (m.deps.len == 0) {
        std.debug.print("No dependencies to install\n", .{});
        return;
    }

    var r = resolver.Resolver.init(allocator);
    defer r.deinit();

    for (m.deps) |dep| {
        const constraint = try types.Constraint.parse(dep.version orelse "*");
        try r.addConstraint(.{
            .package = dep.name,
            .constraint = constraint,
            .source = dep.source,
            .required_by = "root",
        });
    }

    var graph = try r.resolve();
    defer graph.deinit();

    std.debug.print("Resolved {d} packages\n", .{graph.nodes.items.len});

    const home_buf = std.process.getEnvVarOwned(allocator, "HOME") catch "";
    defer if (home_buf.len > 0) allocator.free(home_buf);
    const home = if (home_buf.len > 0) home_buf else ".";
    const store_base = try std.fs.path.join(allocator, &.{ home, ".ara", "store" });
    defer allocator.free(store_base);

    var store = try store_cas.Store.init(allocator, store_base);
    defer store.deinit();
    try store.ensureDirs();

    const node_modules = try std.fs.path.join(allocator, &.{ cwd, "node_modules" });
    defer allocator.free(node_modules);
    std.fs.makeDirAbsolute(node_modules) catch |err| switch (err) {
        error.PathAlreadyExists => {},
        else => return err,
    };

    var pkg_entries = std.ArrayList(lockfile_types.PackageEntry).init(allocator);
    defer pkg_entries.deinit();

    for (graph.nodes.items) |*node| {
        var ver_buf: [64]u8 = undefined;
        const ver_str = std.fmt.bufPrint(&ver_buf, "{}.{}.{}", .{ node.version.major, node.version.minor, node.version.patch }) catch "0.0.0";

        const dep = findDep(m.deps, node.name) orelse {
            std.debug.print("  skipped {s}: no dependency config\n", .{node.name});
            continue;
        };

        var src = createSource(allocator, node.source, dep) catch |err| {
            std.debug.print("  skipped {s}: failed to create source ({s})\n", .{ node.name, @errorName(err) });
            continue;
        };
        defer src.deinit();

        std.debug.print("  fetching {s}@{s}...\n", .{ node.name, ver_str });

        const pkg_content = src.fetch(allocator, .{
            .source = node.source,
            .name = node.name,
            .version = node.version,
        }) catch |err| {
            std.debug.print("  failed to fetch {s}: {s}\n", .{ node.name, @errorName(err) });
            continue;
        };
        defer allocator.free(pkg_content);

        const hash_str = try store.put(pkg_content);
        defer allocator.free(hash_str);

        const pkg_dir = try std.fs.path.join(allocator, &.{ node_modules, node.name });
        defer allocator.free(pkg_dir);
        std.fs.deleteTreeAbsolute(pkg_dir) catch {};
        try std.fs.makeDirAbsolute(pkg_dir);
        extractTarball(allocator, pkg_content, pkg_dir) catch |err| {
            std.debug.print("  failed to extract {s}: {s}\n", .{ node.name, @errorName(err) });
            continue;
        };

        try pkg_entries.append(.{
            .name = node.name,
            .version = ver_str,
            .source = @tagName(node.source),
            .package_hash = hash_str,
        });

        std.debug.print("  ✓ {s}@{s} ({s})\n", .{ node.name, ver_str, hash_str });
    }

    const ts = try currentTimestamp(allocator);
    defer allocator.free(ts);

    const lock_content = try lockfile_gen.generate(allocator, .{
        .graph = .{
            .resolver = "mvs",
            .generated_at = ts,
            .graph_hash = null,
        },
        .packages = pkg_entries.items,
    });
    defer allocator.free(lock_content);

    const lock_path = try std.fs.path.join(allocator, &.{ cwd, "ara.lock" });
    defer allocator.free(lock_path);

    try std.fs.cwd().writeFile(.{ .sub_path = lock_path, .data = lock_content });
    std.debug.print("Lockfile written to ara.lock\n", .{});
}

pub fn run(allocator: std.mem.Allocator, _: []const u8, script: []const u8) !void {
    std.debug.print("running: {s}\n", .{script});

    var child = std.process.Child.init(&.{ "sh", "-c", script }, allocator);
    _ = try child.spawnAndWait();
}
