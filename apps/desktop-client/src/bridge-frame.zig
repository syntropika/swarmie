const std = @import("std");

/// The carrier accepts a JSON string only. Generic byte-slice decoding also
/// instantiates numeric-array conversion and unsupported f128 CRT symbols.
pub const FrameString = struct {
    bytes: []const u8,

    pub fn jsonParse(allocator: std.mem.Allocator, source: anytype, options: std.json.ParseOptions) !@This() {
        return switch (try source.nextAllocMax(allocator, .alloc_always, options.max_value_len.?)) {
            .string, .allocated_string => |bytes| .{ .bytes = bytes },
            else => error.UnexpectedToken,
        };
    }
};

test "carrier frames retain decoded string bytes" {
    const parsed = try std.json.parseFromSlice(struct { frame: FrameString }, std.testing.allocator, "{\"frame\":\"a\\n\\u00e9\"}", .{});
    defer parsed.deinit();
    try std.testing.expectEqualStrings("a\né", parsed.value.frame.bytes);
}

test "carrier frames refuse non-string values" {
    for ([_][]const u8{ "{\"frame\":[65]}", "{\"frame\":12}", "{\"frame\":null}", "{\"frame\":true}" }) |body| {
        try std.testing.expectError(error.UnexpectedToken, std.json.parseFromSlice(struct { frame: FrameString }, std.testing.allocator, body, .{}));
    }
}
