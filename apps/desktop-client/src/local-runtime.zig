const std = @import("std");
const builtin = @import("builtin");
const native = @import("native_sdk");

const Handle = opaque {};
extern fn swarmie_desktop_runtime_start([*:0]const u8) ?*Handle;
extern fn swarmie_desktop_runtime_exchange(*const Handle, [*]const u8, usize, [*]u8, usize) usize;
extern fn swarmie_desktop_runtime_close(*Handle) void;
const command = "swarmie.desktop.runtime";

/// The native host supplies execution and a byte transport to the main WebView.
pub const LocalRuntime = struct {
    handle: ?*Handle = null,
    origin: []const u8 = "zero://app",

    pub fn init(environment: *std.process.Environ.Map) !LocalRuntime {
        const development = environment.get("NATIVE_SDK_FRONTEND_URL");
        if (development) |url| {
            if (!std.mem.eql(u8, url, "http://127.0.0.1:5175/")) return error.UntrustedDesktopOrigin;
        }
        var buffer: [std.Io.Dir.max_path_bytes]u8 = undefined;
        const directory = try native.app_dirs.resolveOne(.{ .name = "swarmie" }, switch (builtin.os.tag) {
            .macos => .macos,
            .windows => .windows,
            .linux => .linux,
            else => return error.UnsupportedPlatform,
        }, .{
            .home = environment.get("HOME"),
            .xdg_state_home = environment.get("XDG_STATE_HOME"),
            .local_app_data = environment.get("LOCALAPPDATA"),
            .app_data = environment.get("APPDATA"),
        }, .state, &buffer);
        var terminated: [std.Io.Dir.max_path_bytes]u8 = undefined;
        const state = try std.fmt.bufPrintZ(&terminated, "{s}/runtime", .{directory});
        return .{ .handle = swarmie_desktop_runtime_start(state), .origin = if (development != null) "http://127.0.0.1:5175" else "zero://app" };
    }

    pub fn handlers(self: *LocalRuntime) [1]native.BridgeHandler {
        return .{.{ .name = command, .context = self, .invoke_fn = exchange }};
    }

    pub fn policies(self: *LocalRuntime, origins: *[1][]const u8) [1]native.BridgeCommandPolicy {
        origins.* = .{self.origin};
        return .{.{ .name = command, .origins = origins }};
    }

    fn exchange(context: *anyopaque, invocation: native.bridge.Invocation, output: []u8) ![]const u8 {
        if (invocation.source.window_id != 1 or !std.mem.eql(u8, invocation.source.webview_label, "main")) return error.UntrustedDesktopView;
        const self: *LocalRuntime = @ptrCast(@alignCast(context));
        const handle = self.handle orelse return "{\"ok\":false,\"error\":\"The native runtime is unavailable.\"}";
        const input = invocation.request.payload;
        const length = swarmie_desktop_runtime_exchange(handle, input.ptr, input.len, output.ptr, output.len);
        if (length == 0) return error.NativeTransportUnavailable;
        return output[0..length];
    }

    pub fn deinit(self: *LocalRuntime) void {
        if (self.handle) |handle| swarmie_desktop_runtime_close(handle);
        self.handle = null;
    }
};
