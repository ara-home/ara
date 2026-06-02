const std = @import("std");
const types = @import("../types.zig");
const Sha256 = std.crypto.hash.sha2.Sha256;

pub const Node = struct {
    name: []const u8,
    source: types.SourceType,
    version: types.Version,
    package_hash: ?[]const u8 = null,
    dependencies: []const []const u8 = &.{},
};

pub const Graph = struct {
    allocator: std.mem.Allocator,
    nodes: std.ArrayListUnmanaged(Node) = .{},

    pub fn init(allocator: std.mem.Allocator) Graph {
        return .{ .allocator = allocator };
    }

    pub fn deinit(self: *Graph) void {
        self.nodes.deinit(self.allocator);
    }

    pub fn addNode(self: *Graph, node: Node) !void {
        try self.nodes.append(self.allocator, node);
    }

    pub fn findNode(self: *Graph, name: []const u8) ?usize {
        for (self.nodes.items, 0..) |n, i| {
            if (std.mem.eql(u8, n.name, name)) return i;
        }
        return null;
    }

    pub fn computeHash(self: *Graph) ![Sha256.digest_length]u8 {
        _ = self;
        return [_]u8{0} ** 32;
    }

    pub fn hasCycles(self: *Graph) bool {
        _ = self;
        return false;
    }
};

test "graph: add and find nodes" {
    var g = Graph.init(std.testing.allocator);
    defer g.deinit();

    try g.addNode(.{
        .name = "zod",
        .source = .npm,
        .version = try types.Version.parse("3.23.8"),
    });
    try g.addNode(.{
        .name = "react",
        .source = .npm,
        .version = try types.Version.parse("18.3.0"),
    });

    try std.testing.expectEqual(@as(usize, 2), g.nodes.items.len);
    try std.testing.expectEqual(@as(usize, 0), g.findNode("zod").?);
    try std.testing.expectEqual(@as(usize, 1), g.findNode("react").?);
    try std.testing.expect(g.findNode("missing") == null);
}

test "graph: hasCycles returns false (stub)" {
    var g = Graph.init(std.testing.allocator);
    defer g.deinit();

    try std.testing.expect(!g.hasCycles());
}

test "graph: computeHash returns zeros (stub)" {
    var g = Graph.init(std.testing.allocator);
    defer g.deinit();

    const hash = try g.computeHash();
    try std.testing.expectEqual(@as(usize, 32), hash.len);
    for (hash) |byte| try std.testing.expectEqual(@as(u8, 0), byte);
}
