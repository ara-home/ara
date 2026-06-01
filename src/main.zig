const std = @import("std");

pub fn main() !void {
    std.debug.print("ara — deterministic package manager\n", .{});
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
