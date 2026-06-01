const std = @import("std");
const profiles = @import("profiles.zig");

pub const Executor = struct {
    allocator: std.mem.Allocator,
    config: profiles.SandboxConfig,

    pub fn init(allocator: std.mem.Allocator, config: profiles.SandboxConfig) Executor {
        return .{ .allocator = allocator, .config = config };
    }

    pub fn execute(self: *Executor, command: []const u8, args: []const []const u8) !void {
        _ = args;
        const child = std.process.Child.init(&.{ "sh", "-c", command }, self.allocator);
        _ = try child.spawnAndWait();
    }

    pub fn dryRun(self: *Executor, command: []const u8) void {
        std.debug.print("[sandbox] would execute: {s}\n", .{command});
        std.debug.print("[sandbox] profile: {s}\n", .{@tagName(self.config.profile)});
    }
};

test "executor: dry run" {
    var ex = Executor.init(std.testing.allocator, profiles.SandboxConfig.forProfile(.restricted));
    ex.dryRun("echo hello");
}
