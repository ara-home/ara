const std = @import("std");
const types = @import("../types.zig");
const Graph = @import("graph.zig").Graph;
const Node = @import("graph.zig").Node;

pub const ConstraintEntry = struct {
    package: []const u8,
    constraint: types.Constraint,
    source: types.SourceType,
    required_by: []const u8,
};

pub const Resolver = struct {
    allocator: std.mem.Allocator,
    constraints: std.ArrayListUnmanaged(ConstraintEntry) = .{},

    pub fn init(allocator: std.mem.Allocator) Resolver {
        return .{ .allocator = allocator };
    }

    pub fn deinit(self: *Resolver) void {
        self.constraints.deinit(self.allocator);
    }

    pub fn addConstraint(self: *Resolver, entry: ConstraintEntry) !void {
        try self.constraints.append(self.allocator, entry);
    }

    pub fn resolve(self: *Resolver) !Graph {
        var graph = Graph.init(self.allocator);
        errdefer graph.deinit();

        var collected = std.StringHashMap(std.ArrayListUnmanaged(types.Version)).init(self.allocator);
        defer {
            var it = collected.iterator();
            while (it.next()) |entry| {
                entry.value_ptr.deinit(self.allocator);
            }
            collected.deinit();
        }

        for (self.constraints.items) |c| {
            if (!collected.contains(c.package)) {
                try collected.put(c.package, .{});
            }
        }

        for (self.constraints.items) |c| {
            var versions = collected.getPtr(c.package) orelse continue;
            try versions.append(self.allocator, try types.Version.parse("0.0.0"));
        }

        if (self.constraints.items.len > 0) {
            try self.buildGraph(&graph, &collected);
        }

        return graph;
    }

    fn selectVersion(constraints: []const ConstraintEntry, package: []const u8) ?types.Version {
        var best: ?types.Version = null;

        for (constraints) |c| {
            if (!std.mem.eql(u8, c.package, package)) continue;

            const candidate = switch (c.constraint) {
                .exact => |v| v,
                .greaterOrEqual => |v| v,
                .greaterThan => |v| v,
                .caret => |v| v,
                .tilde => |v| v,
                .lessOrEqual, .lessThan, .wildcard => continue,
            };

            if (best) |b| {
                if (candidate.order(b) == .gt) {
                    best = candidate;
                }
            } else {
                best = candidate;
            }
        }

        return best;
    }

    fn buildGraph(self: *Resolver, graph: *Graph, collected: anytype) !void {
        _ = collected;
        var seen = std.StringHashMap(void).init(self.allocator);
        defer seen.deinit();

        for (self.constraints.items) |c| {
            if (seen.contains(c.package)) continue;
            try seen.put(c.package, {});

            const selected = selectVersion(self.constraints.items, c.package) orelse continue;
            const parsed = selected;
            const node = Node{
                .name = c.package,
                .source = c.source,
                .version = parsed,
            };
            try graph.addNode(node);
        }
    }
};

test "resolver: resolve single dependency" {
    var r = Resolver.init(std.testing.allocator);
    defer r.deinit();

    try r.addConstraint(.{
        .package = "zod",
        .constraint = try types.Constraint.parse(">=3.0.0"),
        .source = .npm,
        .required_by = "root",
    });

    var graph = try r.resolve();
    defer graph.deinit();

    try std.testing.expectEqual(@as(usize, 1), graph.nodes.items.len);
}

test "resolver: simple MVS selection" {
    var r = Resolver.init(std.testing.allocator);
    defer r.deinit();

    try r.addConstraint(.{
        .package = "c",
        .constraint = try types.Constraint.parse(">=2.0.0"),
        .source = .npm,
        .required_by = "a",
    });
    try r.addConstraint(.{
        .package = "c",
        .constraint = try types.Constraint.parse(">=2.1.0"),
        .source = .npm,
        .required_by = "b",
    });

    var graph = try r.resolve();
    defer graph.deinit();

    try std.testing.expectEqual(@as(usize, 1), graph.nodes.items.len);
    try std.testing.expectEqualStrings("c", graph.nodes.items[0].name);
}
