const std = @import("std");
const Lockfile = @import("types.zig").Lockfile;

pub fn parse(allocator: std.mem.Allocator, content: []const u8) !Lockfile {
    _ = allocator;
    _ = content;
    return Lockfile{};
}

test "lockfile: placeholder" {
    _ = try parse(std.testing.allocator, "");
}
