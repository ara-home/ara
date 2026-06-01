const std = @import("std");
const testing = std.testing;
const Allocator = std.mem.Allocator;

pub const InlineEntry = struct {
    key: []const u8,
    value: Value,
};

pub const Value = union(enum) {
    string: []const u8,
    integer: i64,
    boolean: bool,
    inline_table: []const InlineEntry,

    pub fn asString(self: Value) ?[]const u8 {
        return switch (self) {
            .string => |s| s,
            else => null,
        };
    }

    pub fn asInteger(self: Value) ?i64 {
        return switch (self) {
            .integer => |n| n,
            else => null,
        };
    }

    pub fn asBoolean(self: Value) ?bool {
        return switch (self) {
            .boolean => |b| b,
            else => null,
        };
    }

    pub fn getInline(self: Value, key: []const u8) ?Value {
        switch (self) {
            .inline_table => |entries| {
                for (entries) |e| {
                    if (std.mem.eql(u8, e.key, key)) return e.value;
                }
                return null;
            },
            else => return null,
        }
    }

    pub fn deinit(self: *Value, allocator: Allocator) void {
        switch (self.*) {
            .inline_table => |entries| {
                for (entries) |*e| {
                    const ptr: *const Value = &e.value;
                    const mut: *Value = @constCast(ptr);
                    mut.deinit(allocator);
                }
                allocator.free(std.mem.sliceAsBytes(entries));
            },
            else => {},
        }
    }
};

pub const Entry = struct {
    key: []const u8,
    value: Value,

    pub fn deinit(self: *Entry, allocator: Allocator) void {
        self.value.deinit(allocator);
    }
};

pub const Table = struct {
    name: []const u8,
    entries: std.ArrayListUnmanaged(Entry) = .{},
    tables: std.ArrayListUnmanaged(Table) = .{},
    arrays: std.ArrayListUnmanaged(ArrayOfTables) = .{},

    pub fn deinit(self: *Table, allocator: Allocator) void {
        for (self.entries.items) |*e| e.deinit(allocator);
        self.entries.deinit(allocator);
        for (self.tables.items) |*t| t.deinit(allocator);
        self.tables.deinit(allocator);
        for (self.arrays.items) |*a| a.deinit(allocator);
        self.arrays.deinit(allocator);
    }
};

pub const ArrayOfTables = struct {
    name: []const u8,
    entries: std.ArrayListUnmanaged(Entry) = .{},

    pub fn deinit(self: *ArrayOfTables, allocator: Allocator) void {
        for (self.entries.items) |*e| e.deinit(allocator);
        self.entries.deinit(allocator);
    }
};

pub const Document = struct {
    root_entries: std.ArrayListUnmanaged(Entry) = .{},
    tables: std.ArrayListUnmanaged(Table) = .{},

    pub fn deinit(self: *Document, allocator: Allocator) void {
        for (self.root_entries.items) |*e| e.deinit(allocator);
        self.root_entries.deinit(allocator);
        for (self.tables.items) |*t| t.deinit(allocator);
        self.tables.deinit(allocator);
    }

    pub fn getEntry(self: Document, table_name: ?[]const u8, key: []const u8) ?Value {
        const entries = if (table_name) |name| blk: {
            for (self.tables.items) |t| {
                if (std.mem.eql(u8, t.name, name)) break :blk t.entries;
            }
            return null;
        } else self.root_entries;

        for (entries.items) |e| {
            if (std.mem.eql(u8, e.key, key)) return e.value;
        }
        return null;
    }

    pub fn getArrays(self: *Document, name: []const u8) ?[]ArrayOfTables {
        for (self.tables.items) |*t| {
            if (std.mem.eql(u8, t.name, name)) return t.arrays.items;
        }
        return null;
    }

    pub fn getTable(self: *Document, name: []const u8) ?*Table {
        for (self.tables.items) |*t| {
            if (std.mem.eql(u8, t.name, name)) return t;
        }
        return null;
    }
};

const Parser = struct {
    const Error = error{
        OutOfMemory,
        UnexpectedCharacter,
        EmptyKey,
        UnexpectedEnd,
        InvalidValue,
        UnterminatedString,
        UnterminatedTable,
        Overflow,
        InvalidCharacter,
    };

    allocator: Allocator,
    source: []const u8,
    pos: usize,

    fn init(allocator: Allocator, source: []const u8) Parser {
        return .{ .allocator = allocator, .source = source, .pos = 0 };
    }

    fn skipWhitespace(self: *Parser) void {
        while (self.pos < self.source.len and
            (self.source[self.pos] == ' ' or self.source[self.pos] == '\t' or
            self.source[self.pos] == '\n' or self.source[self.pos] == '\r'))
        {
            self.pos += 1;
        }
    }

    fn skipLine(self: *Parser) void {
        while (self.pos < self.source.len and self.source[self.pos] != '\n') {
            self.pos += 1;
        }
    }

    fn peek(self: *Parser) ?u8 {
        if (self.pos >= self.source.len) return null;
        return self.source[self.pos];
    }

    fn advance(self: *Parser) u8 {
        const c = self.source[self.pos];
        self.pos += 1;
        return c;
    }

    fn expect(self: *Parser, c: u8) Parser.Error!void {
        if (self.peek() != c) return error.UnexpectedCharacter;
        self.pos += 1;
    }

    fn parseString(self: *Parser) Parser.Error![]const u8 {
        const quote = self.advance();
        const start = self.pos;
        while (self.pos < self.source.len and self.source[self.pos] != quote) {
            if (self.source[self.pos] == '\\') {
                self.pos += 1;
            }
            self.pos += 1;
        }
        if (self.pos >= self.source.len) return error.UnterminatedString;
        const result = self.source[start..self.pos];
        self.pos += 1;
        return result;
    }

    fn parseValue(self: *Parser) Parser.Error!Value {
        self.skipWhitespace();
        const c = self.peek() orelse return error.UnexpectedEnd;
        return switch (c) {
            '"' => .{ .string = try self.parseString() },
            '{' => .{ .inline_table = try self.parseInlineTable() },
            't', 'f' => blk: {
                if (std.mem.startsWith(u8, self.source[self.pos..], "true")) {
                    self.pos += 4;
                    break :blk Value{ .boolean = true };
                }
                if (std.mem.startsWith(u8, self.source[self.pos..], "false")) {
                    self.pos += 5;
                    break :blk Value{ .boolean = false };
                }
                return error.InvalidValue;
            },
            '0'...'9', '-' => blk: {
                const start = self.pos;
                if (c == '-') self.pos += 1;
                while (self.pos < self.source.len and std.ascii.isDigit(self.source[self.pos])) {
                    self.pos += 1;
                }
                const num_str = self.source[start..self.pos];
                break :blk Value{ .integer = try std.fmt.parseInt(i64, num_str, 10) };
            },
            else => error.InvalidValue,
        };
    }

    fn parseInlineTable(self: *Parser) Parser.Error![]const InlineEntry {
        try self.expect('{');
        var entries = std.ArrayList(InlineEntry).init(self.allocator);

        while (true) {
            self.skipWhitespace();
            if (self.peek() == '}') {
                self.pos += 1;
                break;
            }
            const key = try self.parseKey();
            self.skipWhitespace();
            try self.expect('=');
            const value = try self.parseValue();
            try entries.append(.{ .key = key, .value = value });
            self.skipWhitespace();
            if (self.peek() == ',') {
                self.pos += 1;
            }
        }

        return entries.toOwnedSlice();
    }

    fn parseKey(self: *Parser) Parser.Error![]const u8 {
        self.skipWhitespace();
        const start = self.pos;
        while (self.pos < self.source.len) {
            const c = self.source[self.pos];
            if (c == '=' or c == '.' or c == ' ' or c == '\t' or c == '\n' or c == '\r') break;
            self.pos += 1;
        }
        if (self.pos == start) return error.EmptyKey;
        return self.source[start..self.pos];
    }

    fn parseEntries(self: *Parser, entries: *std.ArrayListUnmanaged(Entry)) (Parser.Error || Allocator.Error)!void {
        while (self.pos < self.source.len) {
            self.skipWhitespace();
            if (self.pos >= self.source.len) break;

            const c = self.peek() orelse break;
            switch (c) {
                '#' => {
                    self.skipLine();
                },
                '\n', '\r' => {
                    self.pos += 1;
                },
                '[' => return,
                else => {
                    const key = try self.parseKey();
                    self.skipWhitespace();
                    try self.expect('=');
                    const value = try self.parseValue();
                    try entries.append(self.allocator, .{ .key = key, .value = value });

                    self.skipWhitespace();
                    if (self.peek() == '#') self.skipLine();
                },
            }
        }
    }

    fn parseHeaderName(self: *Parser) Parser.Error![]const u8 {
        const start = self.pos;
        while (self.pos < self.source.len and self.source[self.pos] != ']') {
            self.pos += 1;
        }
        if (self.pos >= self.source.len) return error.UnterminatedTable;
        const name = std.mem.trim(u8, self.source[start..self.pos], " \t");
        return name;
    }

    fn parseTableHeader(self: *Parser) Parser.Error![]const u8 {
        try self.expect('[');
        const name = try self.parseHeaderName();
        try self.expect(']');
        return name;
    }

    fn parseArrayHeader(self: *Parser) Parser.Error![]const u8 {
        try self.expect('[');
        try self.expect('[');
        const name = try self.parseHeaderName();
        try self.expect(']');
        try self.expect(']');
        return name;
    }

    fn parseDocument(self: *Parser) (Parser.Error || Allocator.Error)!Document {
        var doc = Document{};

        try self.parseEntries(&doc.root_entries);

        while (self.pos < self.source.len) {
            self.skipWhitespace();
            if (self.pos >= self.source.len) break;
            if (self.peek() == '#') {
                self.skipLine();
                continue;
            }
            if (self.peek() != '[') break;

            const next = self.pos + 1;
            if (next < self.source.len and self.source[next] == '[') {
                const name = try self.parseArrayHeader();
                self.skipWhitespace();
                var arr = ArrayOfTables{ .name = name };
                try self.parseEntries(&arr.entries);

                var found = false;
                for (doc.tables.items) |*t| {
                    if (std.mem.eql(u8, t.name, name)) {
                        try t.arrays.append(self.allocator, arr);
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    var t = Table{ .name = name };
                    try t.arrays.append(self.allocator, arr);
                    try doc.tables.append(self.allocator, t);
                }
            } else {
                const name = try self.parseTableHeader();
                self.skipWhitespace();
                var table = Table{ .name = name };
                try self.parseEntries(&table.entries);
                try doc.tables.append(self.allocator, table);
            }
        }

        return doc;
    }
};

pub fn parse(allocator: Allocator, source: []const u8) (Parser.Error || Allocator.Error)!Document {
    var p = Parser.init(allocator, source);
    return p.parseDocument();
}

test "toml: parse simple key-value" {
    const src =
        \\name = "ara"
        \\version = "1.0.0"
    ;
    var doc = try parse(testing.allocator, src);
    defer doc.deinit(testing.allocator);

    const name = doc.getEntry(null, "name");
    try testing.expect(name != null);
    try testing.expectEqualStrings("ara", name.?.asString().?);

    const version = doc.getEntry(null, "version");
    try testing.expect(version != null);
    try testing.expectEqualStrings("1.0.0", version.?.asString().?);
}

test "toml: parse table" {
    const src =
        \\[project]
        \\name = "my-app"
        \\version = "0.1.0"
    ;
    var doc = try parse(testing.allocator, src);
    defer doc.deinit(testing.allocator);

    const table = doc.getTable("project");
    try testing.expect(table != null);
    try testing.expectEqualStrings("my-app", doc.getEntry("project", "name").?.asString().?);
    try testing.expectEqualStrings("0.1.0", doc.getEntry("project", "version").?.asString().?);
}

test "toml: parse boolean and integer" {
    const src =
        \\hermetic = true
        \\port = 8080
    ;
    var doc = try parse(testing.allocator, src);
    defer doc.deinit(testing.allocator);

    const hermetic = doc.getEntry(null, "hermetic");
    try testing.expectEqual(true, hermetic.?.asBoolean().?);

    const port = doc.getEntry(null, "port");
    try testing.expectEqual(@as(i64, 8080), port.?.asInteger().?);
}

test "toml: parse array of tables" {
    const src =
        \\[[package]]
        \\name = "zod"
        \\version = "3.23.8"
        \\
        \\[[package]]
        \\name = "react"
        \\version = "18.3.0"
    ;
    var doc = try parse(testing.allocator, src);
    defer doc.deinit(testing.allocator);

    const packages = doc.getArrays("package");
    try testing.expect(packages != null);
    try testing.expectEqual(@as(usize, 2), packages.?.len);
}

test "toml: parse with comments" {
    const src =
        \\# this is a comment
        \\name = "ara"
        \\# another comment
        \\version = "1.0.0"
    ;
    var doc = try parse(testing.allocator, src);
    defer doc.deinit(testing.allocator);

    const name = doc.getEntry(null, "name");
    try testing.expectEqualStrings("ara", name.?.asString().?);
}

test "toml: parse inline table" {
    const src =
        \\zod = { source = "npm", version = "3.23.8" }
    ;
    var doc = try parse(testing.allocator, src);
    defer doc.deinit(testing.allocator);

    const val = doc.getEntry(null, "zod");
    try testing.expect(val != null);
    const source = val.?.getInline("source");
    try testing.expectEqualStrings("npm", source.?.asString().?);
    const version = val.?.getInline("version");
    try testing.expectEqualStrings("3.23.8", version.?.asString().?);
}
