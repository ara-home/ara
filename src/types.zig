const std = @import("std");

pub const SourceType = enum {
    workspace,
    local,
    git,
    github,
    registry,
    npm,

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

        if (s.len == 1 and s[0] == '*') {
            return .{ .wildcard = .{ .major = std.math.maxInt(u32), .minor = null } };
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
        const major = std.fmt.parseInt(u32, major_str, 10) catch return .{ .major = 0, .minor = null };
        const minor: ?u32 = if (dot) |d| blk: {
            const rest = s[d + 1 ..];
            const second_dot = std.mem.indexOfScalar(u8, rest, '.');
            if (second_dot) |sd| {
                break :blk std.fmt.parseInt(u32, rest[0..sd], 10) catch null;
            }
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
                if (w.major == std.math.maxInt(u32)) return true;
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

pub const RiskLevel = enum {
    low,
    medium,
    high,
    critical,
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

test "version: invalid inputs" {
    try std.testing.expectError(error.InvalidVersion, Version.parse(""));
    try std.testing.expectError(error.InvalidVersion, Version.parse("1"));
    try std.testing.expectError(error.InvalidVersion, Version.parse("1."));
    try std.testing.expectError(error.InvalidVersion, Version.parse("1.2"));
    try std.testing.expectError(error.InvalidVersion, Version.parse("1.2."));
    try std.testing.expectError(error.InvalidVersion, Version.parse(".1.2.3"));
    try std.testing.expectError(error.InvalidCharacter, Version.parse("a.b.c"));
    const v = try Version.parse("1.2.3.");
    try std.testing.expectEqual(@as(u32, 1), v.major);
    try std.testing.expectEqual(@as(u32, 2), v.minor);
    try std.testing.expectEqual(@as(u32, 3), v.patch);
    try std.testing.expect(v.prerelease == null);
    try std.testing.expect(v.build == null);
}

test "version: overflow returns error" {
    try std.testing.expectError(error.Overflow, Version.parse("9999999999999.0.0"));
}

test "version: prerelease and build edge cases" {
    const v = try Version.parse("1.0.0-0");
    try std.testing.expectEqualStrings("0", v.prerelease.?);

    const v2 = try Version.parse("1.0.0+build");
    try std.testing.expect(v2.prerelease == null);
    try std.testing.expectEqualStrings("build", v2.build.?);

    const v3 = try Version.parse("1.0.0-rc.1+build.42");
    try std.testing.expectEqualStrings("rc.1", v3.prerelease.?);
    try std.testing.expectEqualStrings("build.42", v3.build.?);
}

test "version: order with prerelease" {
    const release = try Version.parse("1.0.0");
    const prerelease = try Version.parse("1.0.0-rc.1");
    try std.testing.expectEqual(.gt, release.order(prerelease));
    try std.testing.expectEqual(.lt, prerelease.order(release));
    const same = try Version.parse("1.0.0-rc.1");
    try std.testing.expectEqual(.eq, prerelease.order(same));
}

test "constraint: tilde" {
    const c = try Constraint.parse("~1.2.3");
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.2.3")));
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.2.9")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("1.3.0")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("2.0.0")));
}

test "constraint: greater than" {
    const c = try Constraint.parse(">1.0.0");
    try std.testing.expect(!c.satisfiedBy(try Version.parse("1.0.0")));
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.0.1")));
}

test "constraint: less than" {
    const c = try Constraint.parse("<2.0.0");
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.9.9")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("2.0.0")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("2.0.1")));
}

test "constraint: less or equal" {
    const c = try Constraint.parse("<=2.0.0");
    try std.testing.expect(c.satisfiedBy(try Version.parse("2.0.0")));
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.0.0")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("2.0.1")));
}

test "constraint: wildcard star" {
    const c = try Constraint.parse("*");
    try std.testing.expect(c.satisfiedBy(try Version.parse("0.0.0")));
    try std.testing.expect(c.satisfiedBy(try Version.parse("99.99.99")));
}

test "constraint: wildcard minor" {
    const c = try Constraint.parse("1.2.x");
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.2.0")));
    try std.testing.expect(c.satisfiedBy(try Version.parse("1.2.99")));
    try std.testing.expect(!c.satisfiedBy(try Version.parse("1.3.0")));
}

test "constraint: invalid inputs" {
    try std.testing.expectError(error.EmptyConstraint, Constraint.parse(""));
    try std.testing.expectError(error.InvalidVersion, Constraint.parse("^"));
    try std.testing.expectError(error.InvalidVersion, Constraint.parse(">"));
    try std.testing.expectError(error.InvalidVersion, Constraint.parse("<"));
    try std.testing.expectError(error.InvalidVersion, Constraint.parse("~"));
    try std.testing.expectError(error.InvalidVersion, Constraint.parse(">="));
    try std.testing.expectError(error.InvalidVersion, Constraint.parse("1.2."));
}

test "constraint: satisfiedBy caret major zero" {
    const c0 = try Constraint.parse("^0.1.2");
    try std.testing.expect(c0.satisfiedBy(try Version.parse("0.1.2")));
    try std.testing.expect(c0.satisfiedBy(try Version.parse("0.1.9")));
    try std.testing.expect(!c0.satisfiedBy(try Version.parse("0.2.0")));
    try std.testing.expect(!c0.satisfiedBy(try Version.parse("1.0.0")));
}

test "source type: all roundtrip" {
    const tags = [_]SourceType{ .workspace, .local, .git, .github, .registry, .npm };
    inline for (tags) |tag| {
        const s = tag.toString();
        try std.testing.expectEqual(tag, try SourceType.fromString(s));
    }
}

test "version: generative roundtrip" {
    var rng = std.Random.DefaultPrng.init(42);
    const rand = rng.random();
    var i: usize = 0;
    while (i < 100) : (i += 1) {
        const major = rand.intRangeAtMost(u32, 0, 100);
        const minor = rand.intRangeAtMost(u32, 0, 100);
        const patch = rand.intRangeAtMost(u32, 0, 100);
        const buf = try std.fmt.allocPrint(std.testing.allocator, "{}.{}.{}", .{ major, minor, patch });
        defer std.testing.allocator.free(buf);
        const parsed = try Version.parse(buf);
        try std.testing.expectEqual(major, parsed.major);
        try std.testing.expectEqual(minor, parsed.minor);
        try std.testing.expectEqual(patch, parsed.patch);
    }
}

test "constraint: generative parse does not crash" {
    var rng = std.Random.DefaultPrng.init(1234);
    const rand = rng.random();
    var i: usize = 0;
    while (i < 500) : (i += 1) {
        const len = rand.intRangeAtMost(usize, 0, 20);
        var buf: [20]u8 = undefined;
        for (0..len) |j| {
            buf[j] = rand.intRangeAtMost(u8, 32, 126);
        }
        const input = buf[0..len];
        _ = Constraint.parse(input) catch {};
    }
}

test "comptime type invariants" {
    comptime {
        try std.testing.expect(@sizeOf(Version) <= 48);
        try std.testing.expect(@sizeOf(Constraint) <= 56);
        try std.testing.expect(@typeInfo(Constraint).Union.fields.len == 8);

        for (std.meta.fields(SourceType)) |f| {
            const tag: SourceType = @enumFromInt(f.value);
            try std.testing.expectEqualStrings(f.name, tag.toString());
        }
    }
}
