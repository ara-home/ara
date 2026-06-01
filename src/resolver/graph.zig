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
