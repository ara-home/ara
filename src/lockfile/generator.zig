const std = @import("std");
const Lockfile = @import("types.zig").Lockfile;

pub fn generate(allocator: std.mem.Allocator, lockfile: Lockfile) ![]u8 {
    _ = lockfile;
    return std.fmt.allocPrint(allocator, "version = 1\n", .{});
}

test "lockfile: generate placeholder" {
    const out = try generate(std.testing.allocator, .{});
    defer std.testing.allocator.free(out);
    try std.testing.expect(out.len > 0);
}
