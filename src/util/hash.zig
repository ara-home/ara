const std = @import("std");
const Sha256 = std.crypto.hash.sha2.Sha256;

pub const Hash = struct {
    algorithm: []const u8,
    value: []u8,

    pub fn compute(bytes: []const u8) [Sha256.digest_length]u8 {
        var result: [Sha256.digest_length]u8 = undefined;
        Sha256.hash(bytes, &result, .{});
        return result;
    }

    pub fn hexEncode(hash: [Sha256.digest_length]u8, out: []u8) []u8 {
        return std.fmt.bufPrint(out, "{s}", .{std.fmt.fmtSliceHexLower(&hash)}) catch unreachable;
    }

    pub fn format(hash: [Sha256.digest_length]u8, allocator: std.mem.Allocator) ![]u8 {
        return std.fmt.allocPrint(allocator, "sha256:{s}", .{std.fmt.fmtSliceHexLower(&hash)});
    }

    pub fn fromHex(hex: []const u8) ![Sha256.digest_length]u8 {
        if (hex.len != Sha256.digest_length * 2) return error.InvalidHashLength;
        var result: [Sha256.digest_length]u8 = undefined;
        _ = try std.fmt.hexToBytes(&result, hex);
        return result;
    }
};

test "hash: compute and hex encode" {
    const data = "hello";
    const h = Hash.compute(data);
    var buf: [64]u8 = undefined;
    const encoded = Hash.hexEncode(h, &buf);
    try std.testing.expectEqualStrings(
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        encoded[0..64],
    );
}

test "hash: format with prefix" {
    const data = "hello";
    const h = Hash.compute(data);
    const formatted = try Hash.format(h, std.testing.allocator);
    defer std.testing.allocator.free(formatted);
    try std.testing.expect(std.mem.startsWith(u8, formatted, "sha256:"));
}
