const std = @import("std");

pub const RpcError = struct {
    code: i32,
    message: []const u8,
};

pub const Request = struct {
    id: u64,
    method: []const u8,
    params: std.json.Value,
};

pub const Response = struct {
    id: u64,
    result: ?std.json.Value,
    @"error": ?RpcError,
};

pub const Method = enum {
    scan,
    verify,
    audit,
    shutdown,
    analyze,

    pub fn toString(self: Method) []const u8 {
        return @tagName(self);
    }
};
