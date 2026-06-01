const std = @import("std");

pub const Graph = struct {
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) Graph {
        return .{ .allocator = allocator };
    }
};

test "graph: placeholder" {
    var g = Graph.init(std.testing.allocator);
    _ = &g;
}
