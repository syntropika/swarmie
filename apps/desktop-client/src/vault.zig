const std = @import("std");
const native = @import("native_sdk");
const Handle = opaque {};
extern fn swarmie_native_start(?*const anyopaque) ?*Handle;
extern fn swarmie_native_connect(*Handle) bool;
extern fn swarmie_native_send(*Handle, [*]const u8, usize) bool;
extern fn swarmie_native_receive(*Handle, [*]u8, usize) isize;
extern fn swarmie_native_close(*Handle) void;
extern fn swarmie_native_disconnect(*Handle) void;
const commands = [_]native.BridgeCommandPolicy{
    .{ .name = "swarmie.native.available" },
    .{ .name = "swarmie.native.connect" },
    .{ .name = "swarmie.native.send" },
    .{ .name = "swarmie.native.receive" },
};

/// Private in-process byte carrier. Tembra owns methods, State and invocation lifecycle.
pub const Vault = struct {
    pub const command_count = commands.len;

    handle: ?*Handle = null,
    handlers: [command_count]native.BridgeHandler = undefined,
    policies: [command_count]native.BridgeCommandPolicy = undefined,
    origins: [1][]const u8 = .{"zero://app"},

    pub fn init(recording: bool, runtime: ?*const anyopaque, origin: []const u8) Vault {
        // SDK session journals retain raw bridge events. Private entry is unavailable under
        // record/replay until that dependency provides qualified sensitive-event redaction.
        return .{ .handle = if (recording) null else swarmie_native_start(runtime), .origins = .{origin} };
    }
    pub fn deinit(self: *Vault) void {
        if (self.handle) |handle| swarmie_native_close(handle);
        self.handle = null;
    }
    pub fn dispatcher(self: *Vault) native.BridgeDispatcher {
        self.policies = commands;
        for (&self.policies) |*policy| policy.origins = &self.origins;
        self.handlers = .{
            .{ .name = "swarmie.native.available", .context = self, .invoke_fn = available },
            .{ .name = "swarmie.native.connect", .context = self, .invoke_fn = connect },
            .{ .name = "swarmie.native.send", .context = self, .invoke_fn = send },
            .{ .name = "swarmie.native.receive", .context = self, .invoke_fn = receive },
        };
        return .{ .policy = .{ .enabled = true, .commands = &self.policies }, .registry = .{ .handlers = &self.handlers } };
    }
    fn owner(context: *anyopaque, invocation: native.bridge.Invocation) !*Vault {
        if (invocation.source.window_id != 1 or !std.mem.eql(u8, invocation.source.webview_label, "main")) return error.PrivateChannelRequired;
        return @ptrCast(@alignCast(context));
    }
    fn available(context: *anyopaque, invocation: native.bridge.Invocation, _: []u8) ![]const u8 {
        const self = try owner(context, invocation);
        return if (self.handle != null) "true" else "false";
    }
    fn connect(context: *anyopaque, invocation: native.bridge.Invocation, _: []u8) ![]const u8 {
        const self = try owner(context, invocation);
        const handle = self.handle orelse return error.PrivateChannelUnavailable;
        if (!swarmie_native_connect(handle)) return error.PrivateChannelUnavailable;
        return "true";
    }
    fn send(context: *anyopaque, invocation: native.bridge.Invocation, _: []u8) ![]const u8 {
        const self = try owner(context, invocation);
        const handle = self.handle orelse return error.PrivateChannelUnavailable;
        const parsed = try std.json.parseFromSlice(struct { frame: []const u8 }, std.heap.page_allocator, invocation.request.payload, .{ .allocate = .alloc_always });
        defer parsed.deinit();
        defer std.crypto.secureZero(u8, @constCast(parsed.value.frame));
        if (!swarmie_native_send(handle, parsed.value.frame.ptr, parsed.value.frame.len)) return error.PrivateChannelRefused;
        return "null";
    }
    fn receive(context: *anyopaque, invocation: native.bridge.Invocation, output: []u8) ![]const u8 {
        const self = try owner(context, invocation);
        const handle = self.handle orelse return error.PrivateChannelUnavailable;
        const bytes = try std.heap.page_allocator.alloc(u8, 400_000);
        defer std.heap.page_allocator.free(bytes);
        defer std.crypto.secureZero(u8, bytes);
        const length = swarmie_native_receive(handle, bytes.ptr, bytes.len);
        if (length < 0) return error.PrivateChannelEnded;
        if (length == 0) return "null";
        const encoded = native.bridge.writeJsonStringValue(output, bytes[0..@intCast(length)]);
        if (encoded.len == 0) {
            swarmie_native_disconnect(handle);
            return error.PrivateFrameOutputTooSmall;
        }
        return encoded;
    }
};
