const std = @import("std");

pub const Profile = enum {
    open,
    restricted,
    hermetic,
    custom,

    pub fn fromString(s: []const u8) !Profile {
        return std.meta.stringToEnum(Profile, s) orelse error.UnknownProfile;
    }
};

pub const FilesystemAccess = struct {
    allowed_paths: []const []const u8 = &.{},
    writable_paths: []const []const u8 = &.{},
};

pub const NetworkAccess = struct {
    enabled: bool = false,
    allowed_hosts: []const []const u8 = &.{},
};

pub const EnvironmentAccess = struct {
    allowed_vars: []const []const u8 = &.{},
};

pub const ProcessAccess = struct {
    allow_spawn: bool = false,
};

pub const ClockAccess = struct {
    deterministic: bool = false,
};

pub const SandboxConfig = struct {
    profile: Profile,
    filesystem: FilesystemAccess = .{},
    network: NetworkAccess = .{},
    environment: EnvironmentAccess = .{},
    process: ProcessAccess = .{},
    clock: ClockAccess = .{},

    pub fn forProfile(profile: Profile) SandboxConfig {
        return switch (profile) {
            .open => .{
                .profile = .open,
                .network = .{ .enabled = true },
                .process = .{ .allow_spawn = true },
                .environment = .{ .allowed_vars = &.{"*"} },
            },
            .restricted => .{
                .profile = .restricted,
                .filesystem = .{ .allowed_paths = &.{"./"} },
            },
            .hermetic => .{
                .profile = .hermetic,
                .clock = .{ .deterministic = true },
            },
            .custom => .{
                .profile = .custom,
            },
        };
    }
};

test "sandbox profiles: open has network and process" {
    const config = SandboxConfig.forProfile(.open);
    try std.testing.expect(config.network.enabled);
    try std.testing.expect(config.process.allow_spawn);
}

test "sandbox profiles: restricted has no network" {
    const config = SandboxConfig.forProfile(.restricted);
    try std.testing.expect(!config.network.enabled);
    try std.testing.expect(!config.process.allow_spawn);
}

test "sandbox profiles: hermetic has deterministic clock" {
    const config = SandboxConfig.forProfile(.hermetic);
    try std.testing.expect(config.clock.deterministic);
}
