const std = @import("std");
const types = @import("../types.zig");
const Manifest = @import("types.zig").Manifest;

pub fn parse(allocator: std.mem.Allocator, content: []const u8) !Manifest {
    _ = allocator;
    _ = content;
    return Manifest{};
}

test "manifest: placeholder" {
    const m = try parse(std.testing.allocator, "");
    _ = m;
}
