const std = @import("std");

pub const Store = struct {
    allocator: std.mem.Allocator,
    base_path: []const u8,

    pub fn init(allocator: std.mem.Allocator, base_path: []const u8) Store {
        return .{ .allocator = allocator, .base_path = base_path };
    }

    pub fn put(self: *Store, bytes: []const u8) ![]u8 {
        _ = self;
        _ = bytes;
        return "";
    }

    pub fn get(self: *Store, hash: []const u8) ![]u8 {
        _ = self;
        _ = hash;
        return "";
    }

    pub fn contains(self: *Store, hash: []const u8) bool {
        _ = self;
        _ = hash;
        return false;
    }
};

test "store: init" {
    var store = Store.init(std.testing.allocator, "/tmp/test-store");
    _ = &store;
}
