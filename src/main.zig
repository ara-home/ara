const std = @import("std");
const commands = @import("cli/commands.zig");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    defer _ = gpa.deinit();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    if (args.len < 2) {
        try printUsage();
        return;
    }

    const cmd = commands.Command.fromString(args[1]);
    const cwd = try std.fs.cwd().realpathAlloc(allocator, ".");
    defer allocator.free(cwd);

    switch (cmd) {
        .install => try commands.install(allocator, cwd),
        .run => {
            if (args.len < 3) {
                std.debug.print("error: missing script name\n", .{});
                return;
            }
            try commands.run(allocator, cwd, args[2]);
        },
        .build => {
            std.debug.print("build: not yet implemented\n", .{});
        },
        .publish => {
            std.debug.print("publish: not yet implemented\n", .{});
        },
        .gc => {
            std.debug.print("gc: not yet implemented\n", .{});
        },
        .trust => {
            if (args.len < 3) {
                std.debug.print("error: missing package name\n", .{});
                return;
            }
            std.debug.print("trust {s}: not yet implemented\n", .{args[2]});
        },
        .unknown => {
            std.debug.print("error: unknown command '{s}'\n", .{args[1]});
            try printUsage();
        },
    }
}

fn printUsage() !void {
    std.debug.print(
        \\ara — deterministic package manager
        \\
        \\Usage:
        \\  ara install         Install dependencies
        \\  ara run <script>    Run a script
        \\  ara build           Build the project
        \\  ara publish         Publish a package
        \\  ara gc              Garbage collect store
        \\  ara trust <pkg>     Trust a package
        \\
    , .{});
}

test {
    _ = @import("manifest/parser.zig");
    _ = @import("lockfile/parser.zig");
    _ = @import("lockfile/generator.zig");
    _ = @import("store/cas.zig");
    _ = @import("resolver/mvs.zig");
    _ = @import("resolver/graph.zig");
    _ = @import("source/workspace.zig");
    _ = @import("source/local.zig");
    _ = @import("source/git.zig");
    _ = @import("source/github.zig");
    _ = @import("source/registry.zig");
    _ = @import("cli/commands.zig");
    _ = @import("sandbox/profiles.zig");
    _ = @import("sandbox/executor.zig");
    _ = @import("util/hash.zig");
    _ = @import("util/http.zig");
    _ = @import("util/toml.zig");
    _ = @import("types.zig");
    _ = @import("manifest/types.zig");
    _ = @import("lockfile/types.zig");
}
