const std = @import("std");
const hash = @import("util/hash.zig");

pub const SourceType = enum {
    workspace,
    local,
    git,
    github,
    registry,

    pub fn fromString(s: []const u8) !SourceType {
        return std.meta.stringToEnum(SourceType, s) orelse error.UnknownSourceType;
    }

    pub fn toString(self: SourceType) []const u8 {
        return @tagName(self);
    }
};

pub const Version = struct {
    major: u32,
    minor: u32,
    patch: u32,
    prerelease: ?[]const u8 = null,
    build: ?[]const u8 = null,

    pub fn parse(s: []const u8) !Version {
        var rest = s;
        const major = try parseNum(&rest);
        if (rest.len == 0 or rest[0] != '.') return error.InvalidVersion;
        rest = rest[1..];
        const minor = try parseNum(&rest);
        if (rest.len == 0 or rest[0] != '.') return error.InvalidVersion;
        rest = rest[1..];
        const patch = try parseNum(&rest);

        var prerelease: ?[]const u8 = null;
        var build_meta: ?[]const u8 = null;

        if (rest.len > 0 and rest[0] == '-') {
            const end = std.mem.indexOfScalar(u8, rest, '+') orelse rest.len;
            prerelease = rest[1..end];
            rest = rest[end..];
        }
        if (rest.len > 0 and rest[0] == '+') {
            build_meta = rest[1..];
        }

        return .{
            .major = major,
            .minor = minor,
            .patch = patch,
            .prerelease = prerelease,
            .build = build_meta,
        };
    }

    fn parseNum(s: *[]const u8) !u32 {
        const end = std.mem.indexOfAny(u8, s.*, ".-+") orelse s.len;
        if (end == 0) return error.InvalidVersion;
        const num = try std.fmt.parseInt(u32, s.*[0..end], 10);
        s.* = s.*[end..];
        return num;
    }

    pub fn format(self: Version, allocator: std.mem.Allocator) ![]u8 {
        var result = try std.fmt.allocPrint(allocator, "{}.{}.{}", .{ self.major, self.minor, self.patch });
        if (self.prerelease) |pre| {
            const new = try std.fmt.allocPrint(allocator, "{s}-{s}", .{ result, pre });
            allocator.free(result);
            result = new;
        }
        if (self.build) |b| {
            const new = try std.fmt.allocPrint(allocator, "{s}+{s}", .{ result, b });
            allocator.free(result);
            result = new;
        }
        return result;
    }

    pub fn order(self: Version, other: Version) std.math.Order {
        if (self.major != other.major) return std.math.order(self.major, other.major);
        if (self.minor != other.minor) return std.math.order(self.minor, other.minor);
        if (self.patch != other.patch) return std.math.order(self.patch, other.patch);

        if (self.prerelease == null and other.prerelease != null) return .gt;
        if (self.prerelease != null and other.prerelease == null) return .lt;
        return .eq;
    }
};

pub const WildcardParts = struct { major: u32, minor: ?u32 };

pub const Constraint = union(enum) {
    exact: Version,
    caret: Version,
    tilde: Version,
    greaterOrEqual: Version,
    greaterThan: Version,
    lessOrEqual: Version,
    lessThan: Version,
    wildcard: WildcardParts,

    pub fn parse(s: []const u8) !Constraint {
        if (s.len == 0) return error.EmptyConstraint;

        if (s[0] == '^') return .{ .caret = try Version.parse(s[1..]) };
        if (s[0] == '~') return .{ .tilde = try Version.parse(s[1..]) };
        if (s[0] == '>') {
            if (s.len > 1 and s[1] == '=') return .{ .greaterOrEqual = try Version.parse(s[2..]) };
            return .{ .greaterThan = try Version.parse(s[1..]) };
        }
        if (s[0] == '<') {
            if (s.len > 1 and s[1] == '=') return .{ .lessOrEqual = try Version.parse(s[2..]) };
            return .{ .lessThan = try Version.parse(s[1..]) };
        }

        if (std.mem.indexOfScalar(u8, s, 'x')) |_| {
            const parts = splitWildcard(s);
            return .{ .wildcard = parts };
        }

        return .{ .exact = try Version.parse(s) };
    }

    fn splitWildcard(s: []const u8) WildcardParts {
        const dot = std.mem.indexOfScalar(u8, s, '.');
        const major_str = if (dot) |d| s[0..d] else s;
        const major = std.fmt.parseInt(u32, major_str, 10) catch unreachable;
        const minor: ?u32 = if (dot) |d| blk: {
            const rest = s[d + 1 ..];
            if (rest.len == 0 or std.mem.eql(u8, rest, "x")) break :blk null;
            break :blk std.fmt.parseInt(u32, rest, 10) catch null;
        } else null;
        return .{ .major = major, .minor = minor };
    }

    pub fn satisfiedBy(self: Constraint, version: Version) bool {
        return switch (self) {
            .exact => |v| v.order(version) == .eq,
            .caret => |v| {
                if (version.major != v.major) return false;
                if (v.major == 0) {
                    if (v.minor != version.minor) return false;
                    return version.patch >= v.patch;
                }
                return version.order(v) != .lt;
            },
            .tilde => |v| {
                if (version.major != v.major) return false;
                if (version.minor != v.minor) return false;
                return version.patch >= v.patch;
            },
            .greaterOrEqual => |v| version.order(v) != .lt,
            .greaterThan => |v| version.order(v) == .gt,
            .lessOrEqual => |v| version.order(v) != .gt,
            .lessThan => |v| version.order(v) == .lt,
            .wildcard => |w| {
                if (version.major != w.major) return false;
                if (w.minor) |m| if (version.minor != m) return false;
                return true;
            },
        };
    }
};

pub const PackageIdentity = struct {
    source: SourceType,
    name: []const u8,
    version: Version,
    content_hash: ?[]const u8 = null,
};

pub const Dependency = struct {
    name: []const u8,
    source: SourceType,
    constraint: Constraint,

    source_config: SourceConfig = .{},
};

pub const SourceConfig = struct {
    repo: ?[]const u8 = null,
    url: ?[]const u8 = null,
    commit: ?[]const u8 = null,
    path: ?[]const u8 = null,
    package: ?[]const u8 = null,
};

pub const RiskLevel = enum {
    low,
    medium,
    high,
    critical,
};

pub const SandboxProfile = enum {
    open,
    restricted,
    hermetic,
    custom,
};

pub const Error = error{
    UnknownSourceType,
    InvalidVersion,
    InvalidConstraint,
    InvalidHashLength,
    EmptyConstraint,
    ManifestNotFound,
    InvalidManifest,
    LockfileNotFound,
    InvalidLockfile,
    StoreNotFound,
    SourceNotFound,
    ResolutionFailed,
    NetworkError,
    IntegrityMismatch,
    GitError,
    PermissionDenied,
    ResourceBusy,
    OutOfMemory,
    InvalidPath,
    DuplicatePackage,
    CycleDetected,
    UnresolvedReference,
    IpcError,
    SandboxError,
};

test "version: parse and compare" {
    const v1 = try Version.parse("1.2.3");
    try std.testing.expectEqual(@as(u32, 1), v1.major);
    try std.testing.expectEqual(@as(u32, 2), v1.minor);
    try std.testing.expectEqual(@as(u32, 3), v1.patch);

    const v2 = try Version.parse("2.0.0");
    try std.testing.expectEqual(.lt, v1.order(v2));
    try std.testing.expectEqual(.gt, v2.order(v1));
    try std.testing.expectEqual(.eq, v1.order(v1));

    const v3 = try Version.parse("1.2.3-alpha.1+build.42");
    try std.testing.expectEqualStrings("alpha.1", v3.prerelease.?);
    try std.testing.expectEqualStrings("build.42", v3.build.?);
}

test "constraint: exact match" {
    const c = try Constraint.parse("1.2.3");
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.2.3")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("1.2.4")));
}

test "constraint: caret" {
    const c = try Constraint.parse("^1.2.3");
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.5.0")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("2.0.0")));
}

test "constraint: wildcard" {
    const c = try Constraint.parse("1.x");
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.2.3")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("2.0.0")));
}

test "constraint: greater or equal" {
    const c = try Constraint.parse(">=2.0.0");
    try std.testing.expect(c.satisfiedBy(try Version.parse("2.0.0")));
    try std.testing.expect(c.satisfiedBy(try Version.parse("3.0.0")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("1.9.9")));
}

test "source type: parse and format" {
    try std.testing.expectEqual(.workspace, try SourceType.fromString("workspace"));
    try std.testing.expectEqualStrings("registry", SourceType.registry.toString());
    try std.testing.expectError(error.UnknownSourceType, SourceType.fromString("unknown"));
}
