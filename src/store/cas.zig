const std = @import("std");
const hash = @import("../util/hash.zig");

pub const Store = struct {
    allocator: std.mem.Allocator,
    base_path: []const u8,

    pub fn init(allocator: std.mem.Allocator, base_path: []const u8) !Store {
        return .{ .allocator = allocator, .base_path = try allocator.dupe(u8, base_path) };
    }

    pub fn deinit(self: *Store) void {
        self.allocator.free(self.base_path);
    }

    pub fn ensureDirs(self: *Store) !void {
        try std.fs.cwd().makePath(self.base_path);
        const dirs = [_][]const u8{ "objects", "graphs", "snapshots", "cache", "temp" };
        for (dirs) |d| {
            const path = try std.fs.path.join(self.allocator, &.{ self.base_path, d });
            defer self.allocator.free(path);
            std.fs.makeDirAbsolute(path) catch |err| switch (err) {
                error.PathAlreadyExists => continue,
                else => return err,
            };
        }
    }

    fn objectPath(self: *Store, hash_str: []const u8) ![]u8 {
        return std.fs.path.join(self.allocator, &.{ self.base_path, "objects", hash_str });
    }

    pub fn put(self: *Store, bytes: []const u8) ![]u8 {
        const content_hash = hash.Hash.compute(bytes);
        var hex_buf: [64]u8 = undefined;
        const hex = hash.Hash.hexEncode(content_hash, &hex_buf);
        const hash_str = try std.fmt.allocPrint(self.allocator, "sha256-{s}", .{hex});

        const path = try self.objectPath(hash_str);
        defer self.allocator.free(path);

        const file = std.fs.createFileAbsolute(path, .{}) catch |err| switch (err) {
            error.PathAlreadyExists => return hash_str,
            else => return err,
        };
        defer file.close();

        try file.writeAll(bytes);
        return hash_str;
    }

    pub fn get(self: *Store, hash_str: []const u8) !?[]u8 {
        const path = try self.objectPath(hash_str);
        defer self.allocator.free(path);

        const file = std.fs.openFileAbsolute(path, .{ .mode = .read_only }) catch |err| switch (err) {
            error.FileNotFound => return null,
            else => return err,
        };
        defer file.close();

        const stat = try file.stat();
        const buf = try self.allocator.alloc(u8, @intCast(stat.size));
        errdefer self.allocator.free(buf);
        _ = try file.readAll(buf);
        return buf;
    }

    pub fn contains(self: *Store, hash_str: []const u8) bool {
        const path = self.objectPath(hash_str) catch return false;
        defer self.allocator.free(path);
        return std.fs.accessAbsolute(path, .{}) != null;
    }

    pub fn remove(self: *Store, hash_str: []const u8) !void {
        const path = try self.objectPath(hash_str);
        defer self.allocator.free(path);
        try std.fs.deleteFileAbsolute(path);
    }

    pub fn graphPath(self: *Store, graph_hash: []const u8) ![]u8 {
        return std.fs.path.join(self.allocator, &.{ self.base_path, "graphs", graph_hash });
    }

    pub fn putGraph(self: *Store, graph_bytes: []const u8) ![]u8 {
        const ghash = hash.Hash.compute(graph_bytes);
        var hex_buf: [64]u8 = undefined;
        const hex = hash.Hash.hexEncode(ghash, &hex_buf);
        const hash_str = try std.fmt.allocPrint(self.allocator, "graph-{s}", .{hex});

        const path = try self.graphPath(hash_str);
        defer self.allocator.free(path);

        const file = std.fs.createFileAbsolute(path, .{}) catch |err| switch (err) {
            error.PathAlreadyExists => return hash_str,
            else => return err,
        };
        defer file.close();
        try file.writeAll(graph_bytes);
        return hash_str;
    }
};

test "store: put and get roundtrip" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();

    const store_path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(store_path);

    var store = try Store.init(std.testing.allocator, store_path);
    defer store.deinit();
    try store.ensureDirs();

    const hash_str = try store.put("hello");
    try std.testing.expect(std.mem.startsWith(u8, hash_str, "sha256-"));

    const data = try store.get(hash_str);
    try std.testing.expect(data != null);
    try std.testing.expectEqualStrings("hello", data.?);
    std.testing.allocator.free(data.?);
    std.testing.allocator.free(hash_str);
}

test "store: deduplication" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();

    const store_path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(store_path);

    var store = try Store.init(std.testing.allocator, store_path);
    defer store.deinit();
    try store.ensureDirs();

    const h1 = try store.put("same");
    const h2 = try store.put("same");
    try std.testing.expectEqualStrings(h1, h2);
    std.testing.allocator.free(h1);
    std.testing.allocator.free(h2);
}

test "store: not found returns null" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();

    const store_path = try tmp.dir.realpathAlloc(std.testing.allocator, ".");
    defer std.testing.allocator.free(store_path);

    var store = try Store.init(std.testing.allocator, store_path);
    defer store.deinit();
    try store.ensureDirs();

    const data = try store.get("sha256-nonexistent");
    try std.testing.expect(data == null);
}
