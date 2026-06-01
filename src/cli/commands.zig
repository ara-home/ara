const std = @import("std");

pub fn install(allocator: std.mem.Allocator) !void {
    _ = allocator;
}

pub fn run(allocator: std.mem.Allocator, script: []const u8) !void {
    _ = allocator;
    _ = script;
}

test "cli: placeholder" {
    try install(std.testing.allocator);
}
