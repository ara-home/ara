const std = @import("std");
const types = @import("../types.zig");
const http = @import("../util/http.zig");

pub const RegistrySource = struct {
    allocator: std.mem.Allocator,
    registry_url: []const u8,

    pub fn init(allocator: std.mem.Allocator, registry_url: []const u8) !RegistrySource {
        return .{ .allocator = allocator, .registry_url = try allocator.dupe(u8, registry_url) };
    }

    pub fn deinit(self: *RegistrySource) void {
        self.allocator.free(self.registry_url);
    }

    pub fn resolve(self: RegistrySource, a: std.mem.Allocator, name: []const u8) ![]const u8 {
        const url = try std.fmt.allocPrint(a, "{s}/{s}", .{ self.registry_url, name });
        defer a.free(url);

        var client = http.HttpClient.init(a);
        defer client.deinit();

        var resp = try client.get(url);
        defer resp.deinit(a);

        const parsed = try std.json.parseFromSlice(std.json.Value, a, resp.body, .{});
        defer parsed.deinit();

        const root = parsed.value;
        const versions = root.object.get("versions") orelse return error.PackageNotFound;
        var latest: ?[]const u8 = null;
        var latest_key: []const u8 = "";
        var iter = versions.object.iterator();
        while (iter.next()) |entry| {
            const ver = types.Version.parse(entry.key_ptr.*) catch continue;
            if (latest) |_| {
                const current = types.Version.parse(latest_key) catch continue;
                if (ver.order(current) == .gt) {
                    latest_key = entry.key_ptr.*;
                    latest = entry.key_ptr.*;
                }
            } else {
                latest_key = entry.key_ptr.*;
                latest = entry.key_ptr.*;
            }
        }

        return a.dupe(u8, latest_key);
    }

    pub fn fetch(self: RegistrySource, a: std.mem.Allocator, id: types.PackageIdentity) ![]u8 {
        var ver_buf: [64]u8 = undefined;
        const ver_str = try std.fmt.bufPrint(&ver_buf, "{}.{}.{}", .{ id.version.major, id.version.minor, id.version.patch });

        const tarball_url = try std.fmt.allocPrint(a, "{s}/{s}/-/{s}-{s}.tgz", .{ self.registry_url, id.name, id.name, ver_str });
        defer a.free(tarball_url);

        var client = http.HttpClient.init(a);
        defer client.deinit();

        var resp = try client.get(tarball_url);
        const body = resp.body;
        resp.body = &.{};
        return body;
    }
};
