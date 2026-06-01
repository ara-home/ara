const std = @import("std");

pub const Resolver = struct {
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) Resolver {
        return .{ .allocator = allocator };
    }
};

test "resolver: placeholder" {
    var r = Resolver.init(std.testing.allocator);
    _ = &r;
}
