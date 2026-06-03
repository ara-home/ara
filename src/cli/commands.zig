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
    analyze,
    audit,
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

// ── ara-sec IPC helpers ────────────────────────────────────────

const ara_sec_dev_paths = [_][]const u8{
    "target/debug/ara-sec",
    "ara-sec/target/debug/ara-sec",
};

fn findAraSecBinary(allocator: std.mem.Allocator) ![]u8 {
    // 1. Same directory as the ara binary (release / .bin/)
    if (std.fs.selfExeDirPathAlloc(allocator)) |dir| {
        defer allocator.free(dir);
        const path = try std.fs.path.join(allocator, &.{ dir, "ara-sec" });
        if (std.fs.accessAbsolute(path, .{})) |_| {
            return path;
        } else |_| {
            allocator.free(path);
        }
    } else |_| {}
    // 2. Dev mode: workspace target, then local target
    for (ara_sec_dev_paths) |rel| {
        const abs = std.fs.cwd().realpathAlloc(allocator, rel) catch continue;
        return abs;
    }
    return error.AraSecNotFound;
}

fn callAraSec(allocator: std.mem.Allocator, binary: []const u8, method: []const u8, params: std.json.Value) !std.json.Parsed(std.json.Value) {
    var child = std.process.Child.init(&.{ binary }, allocator);
    child.stdin_behavior = .Pipe;
    child.stdout_behavior = .Pipe;
    child.stderr_behavior = .Inherit;
    try child.spawn();
    errdefer {
        _ = child.wait() catch {};
    }

    const stdin = child.stdin.?.writer();
    const stdout = child.stdout.?.reader();

    // Build request
    var req_obj = std.json.ObjectMap.init(allocator);
    defer req_obj.deinit();
    try req_obj.put("id", std.json.Value{ .integer = 1 });
    try req_obj.put("method", std.json.Value{ .string = method });
    try req_obj.put("params", params);

    var buf = std.ArrayList(u8).init(allocator);
    defer buf.deinit();
    try std.json.stringify(std.json.Value{ .object = req_obj }, .{}, buf.writer());
    try buf.append('\n');

    stdin.writeAll(buf.items) catch {
        _ = child.wait() catch {};
        return error.ConnectionClosed;
    };

    // Read response
    var line_buf: [65536]u8 = undefined;
    const line = stdout.readUntilDelimiterOrEof(&line_buf, '\n') catch {
        _ = child.wait() catch {};
        return error.ConnectionClosed;
    } orelse {
        _ = child.wait() catch {};
        return error.ConnectionClosed;
    };

    var parsed = try std.json.parseFromSlice(std.json.Value, allocator, line, .{});
    errdefer parsed.deinit();

    // Send shutdown
    var shutdown_obj = std.json.ObjectMap.init(allocator);
    defer shutdown_obj.deinit();
    try shutdown_obj.put("id", std.json.Value{ .integer = 2 });
    try shutdown_obj.put("method", std.json.Value{ .string = "shutdown" });
    try shutdown_obj.put("params", std.json.Value{ .null = {} });

    var shutdown_buf = std.ArrayList(u8).init(allocator);
    defer shutdown_buf.deinit();
    try std.json.stringify(std.json.Value{ .object = shutdown_obj }, .{}, shutdown_buf.writer());
    try shutdown_buf.append('\n');
    stdin.writeAll(shutdown_buf.items) catch {};
    _ = child.wait() catch {};

    return parsed;
}

fn severityColor(severity: []const u8) []const u8 {
    if (std.mem.eql(u8, severity, "critical")) return "\x1b[31;1m"; // red bold
    if (std.mem.eql(u8, severity, "high")) return "\x1b[31m"; // red
    if (std.mem.eql(u8, severity, "medium")) return "\x1b[33m"; // yellow
    if (std.mem.eql(u8, severity, "low")) return "\x1b[36m"; // cyan
    return "\x1b[0m";
}

fn severityLabel(severity: []const u8) []const u8 {
    if (std.mem.eql(u8, severity, "critical")) return "CRITICAL";
    if (std.mem.eql(u8, severity, "high")) return "HIGH";
    if (std.mem.eql(u8, severity, "medium")) return "MEDIUM";
    if (std.mem.eql(u8, severity, "low")) return "LOW";
    return "UNKNOWN";
}

fn printFindings(findings: std.json.Array, risk_level: std.json.Value) void {
    const reset = "\x1b[0m";

    for (findings.items) |item| {
        const obj = item.object;
        const severity = obj.get("severity").?.string;
        const pattern = obj.get("pattern").?.string;
        const location = if (obj.get("location")) |loc| loc.string else "-";
        const description = obj.get("description").?.string;

        const color = severityColor(severity);
        const label = severityLabel(severity);
        std.debug.print("  {s}{s:>8}{s}  {s:<20}  {s:<25}  {s}\n", .{ color, label, reset, pattern, location, description });
    }

    const rl = if (risk_level == .string) risk_level.string else "unknown";
    const rl_label = severityLabel(rl);
    const rl_color = severityColor(rl);
    std.debug.print("\n  Risk level: {s}{s}{s}\n", .{ rl_color, rl_label, reset });
}

// ── analyze command ────────────────────────────────────────────

pub fn analyzeCommand(allocator: std.mem.Allocator, _: []const u8, path_arg: ?[]const u8) !void {
    const binary = findAraSecBinary(allocator) catch {
        std.debug.print("error: ara-sec binary not found. Run `make build-sec` first.\n", .{});
        return;
    };
    defer allocator.free(binary);

    const pkg_path = path_arg orelse ".";
    const abs_path = try std.fs.cwd().realpathAlloc(allocator, pkg_path);
    defer allocator.free(abs_path);

    std.debug.print("Analyzing {s}...\n\n", .{abs_path});

    var params_obj = std.json.ObjectMap.init(allocator);
    defer params_obj.deinit();
    try params_obj.put("package_path", std.json.Value{ .string = abs_path });

    const parsed = callAraSec(allocator, binary, "analyze", std.json.Value{ .object = params_obj }) catch |err| {
        std.debug.print("error: analysis failed ({s})\n", .{@errorName(err)});
        return;
    };
    defer parsed.deinit();

    const root = parsed.value;
    const result = root.object.get("result") orelse {
        std.debug.print("error: unexpected response format\n", .{});
        return;
    };

    const risk_level = result.object.get("risk_level") orelse std.json.Value{ .string = "unknown" };
    const findings = result.object.get("findings") orelse std.json.Value{ .array = std.json.Array.init(allocator) };

    const arr = findings.array;
    if (arr.items.len == 0) {
        std.debug.print("  No suspicious patterns detected.\n", .{});
    } else {
        printFindings(arr, risk_level);
    }
}

// ── audit command ──────────────────────────────────────────────

pub fn auditCommand(allocator: std.mem.Allocator, _: []const u8, path_arg: ?[]const u8) !void {
    const binary = findAraSecBinary(allocator) catch {
        std.debug.print("error: ara-sec binary not found. Run `make build-sec` first.\n", .{});
        return;
    };
    defer allocator.free(binary);

    const pkg_path = path_arg orelse ".";
    const abs_path = try std.fs.cwd().realpathAlloc(allocator, pkg_path);
    defer allocator.free(abs_path);

    std.debug.print("Auditing {s}...\n\n", .{abs_path});

    var params_obj = std.json.ObjectMap.init(allocator);
    defer params_obj.deinit();
    try params_obj.put("package_path", std.json.Value{ .string = abs_path });

    const parsed = callAraSec(allocator, binary, "audit", std.json.Value{ .object = params_obj }) catch |err| {
        std.debug.print("error: audit failed ({s})\n", .{@errorName(err)});
        return;
    };
    defer parsed.deinit();

    const root = parsed.value;
    const result = root.object.get("result") orelse {
        std.debug.print("error: unexpected response format\n", .{});
        return;
    };
    const report = result.object.get("report") orelse {
        std.debug.print("error: unexpected response format\n", .{});
        return;
    };

    const risk_level = report.object.get("risk_level") orelse std.json.Value{ .string = "unknown" };
    const findings = report.object.get("findings") orelse std.json.Value{ .array = std.json.Array.init(allocator) };
    const summary = if (report.object.get("summary")) |s| s.string else "No summary.";

    const arr = findings.array;
    if (arr.items.len == 0) {
        std.debug.print("  No suspicious patterns detected.\n", .{});
    } else {
        printFindings(arr, risk_level);
    }

    std.debug.print("\n  Summary: {s}\n", .{summary});
}

// ── install command ────────────────────────────────────────────

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

    // Try to find ara-sec binary for install-time analysis (non-fatal if not found)
    const sec_binary = findAraSecBinary(allocator) catch null;
    defer if (sec_binary) |s| allocator.free(s);

    const sec_binary_path = sec_binary orelse "";

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

        std.debug.print("  ✓ {s}@{s} ({s})", .{ node.name, ver_str, hash_str });

        // Analyze package after extraction
        if (sec_binary_path.len > 0) {
            var sec_params = std.json.ObjectMap.init(allocator);
            defer sec_params.deinit();
            try sec_params.put("package_path", std.json.Value{ .string = pkg_dir });

            const parsed = callAraSec(allocator, sec_binary_path, "analyze", std.json.Value{ .object = sec_params }) catch {
                std.debug.print("\n", .{});
                continue;
            };
            defer parsed.deinit();

            if (parsed.value.object.get("result")) |result| {
                const findings = if (result.object.get("findings")) |f| f.array else std.json.Array.init(allocator);
                if (findings.items.len > 0) {
                    const rl = if (result.object.get("risk_level")) |rl| rl.string else "unknown";
                    const by_severity = counts: {
                        var ct: u32 = 0;
                        for (findings.items) |fi| {
                            _ = fi;
                            ct += 1;
                        }
                        break :counts ct;
                    };
                    _ = by_severity;
                    const first_desc = findings.items[0].object.get("description").?.string;
                    const first_loc = if (findings.items[0].object.get("location")) |loc| loc.string else "";
                    std.debug.print(" ⚠  {d} finding(s) ({s}) — {s} in {s}", .{ findings.items.len, rl, first_desc, first_loc });
                }
            }
        }

        std.debug.print("\n", .{});
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
