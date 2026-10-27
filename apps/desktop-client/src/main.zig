const std = @import("std");
const builtin = @import("builtin");
const native_sdk = @import("native_sdk");
const runner = @import("runner");
const LocalRuntime = @import("local-runtime.zig").LocalRuntime;
const Updates = @import("updates.zig").Updates;

pub const panic = std.debug.FullPanic(native_sdk.debug.capturePanic);

const frontend: native_sdk.frontend.Config = .{ .dist = "dist" };
const origins = [_][]const u8{ "zero://app", "http://127.0.0.1:5175" };

/// The desktop host loads the existing client; its plugins continue to own presentation state.
const Desktop = struct {
    environment: *std.process.Environ.Map,
    updates: Updates = .{},
    local_runtime: LocalRuntime = .{},
    frontend_config: native_sdk.frontend.Config = frontend,
    asset_root: [std.Io.Dir.max_path_bytes]u8 = undefined,

    fn app(self: *Desktop) native_sdk.App {
        return .{
            .context = self,
            .name = "swarmie",
            .source = native_sdk.frontend.productionSource(frontend),
            .source_fn = source,
            .start_fn = start,
            .event_fn = event,
        };
    }

    fn start(context: *anyopaque, runtime: *native_sdk.Runtime) anyerror!void {
        const self: *Desktop = @ptrCast(@alignCast(context));
        try self.updates.start(runtime);
    }

    fn event(context: *anyopaque, runtime: *native_sdk.Runtime, value: native_sdk.Event) anyerror!void {
        const self: *Desktop = @ptrCast(@alignCast(context));
        try self.updates.event(runtime, value);
    }

    fn source(context: *anyopaque) anyerror!native_sdk.WebViewSource {
        const self: *Desktop = @ptrCast(@alignCast(context));
        return native_sdk.frontend.sourceFromEnv(self.environment, self.frontend_config);
    }

    fn locateAssets(self: *Desktop, io: std.Io) !void {
        if (builtin.os.tag == .macos or self.environment.contains("NATIVE_SDK_FRONTEND_URL")) return;
        var executable: [std.Io.Dir.max_path_bytes]u8 = undefined;
        const length = try std.process.executablePath(io, &executable);
        const directory = std.fs.path.dirname(executable[0..length]) orelse return;
        for ([_][]const u8{ "resources/dist", "../resources/dist" }) |relative| {
            const candidate = try std.fmt.bufPrint(&self.asset_root, "{s}/{s}", .{ directory, relative });
            var index: [std.Io.Dir.max_path_bytes]u8 = undefined;
            const file = try std.fmt.bufPrint(&index, "{s}/index.html", .{candidate});
            std.Io.Dir.cwd().access(io, file, .{}) catch continue;
            self.frontend_config.dist = candidate;
            return;
        }
    }
};

pub fn main(init: std.process.Init) !void {
    var desktop = Desktop{ .environment = init.environ_map };
    try desktop.locateAssets(init.io);
    desktop.updates = Updates.init(init.environ_map.contains("NATIVE_SDK_FRONTEND_URL"));
    defer desktop.updates.deinit();
    desktop.local_runtime = try LocalRuntime.init(init.environ_map);
    defer desktop.local_runtime.deinit();
    var runtime_origins: [1][]const u8 = undefined;
    const runtime_policies = desktop.local_runtime.policies(&runtime_origins);
    const runtime_handlers = desktop.local_runtime.handlers();
    try runner.runWithOptions(desktop.app(), .{
        .app_name = "swarmie",
        .window_title = "Swarmie",
        .bundle_id = "app.swarmie.desktop",
        .icon_path = "assets/icon.png",
        .default_frame = native_sdk.geometry.RectF.init(0, 0, 1280, 800),
        .security = .{ .navigation = .{ .allowed_origins = &origins } },
        .bridge = .{ .policy = .{ .enabled = true, .commands = &runtime_policies }, .registry = .{ .handlers = &runtime_handlers } },
    }, init);
}

test "installed desktop serves bundled client assets without a development server" {
    var environment = std.process.Environ.Map.init(std.testing.allocator);
    defer environment.deinit();
    var desktop = Desktop{ .environment = &environment };
    const loaded = try Desktop.source(&desktop);
    try std.testing.expectEqual(native_sdk.WebViewSourceKind.assets, loaded.kind);
    try std.testing.expectEqualStrings("dist", loaded.asset_options.?.root_path);
    try std.testing.expectEqualStrings("zero://app", loaded.asset_options.?.origin);
}

test "managed development loads its server through the same desktop host" {
    var environment = std.process.Environ.Map.init(std.testing.allocator);
    defer environment.deinit();
    try environment.put("NATIVE_SDK_FRONTEND_URL", "http://127.0.0.1:5175/");
    var desktop = Desktop{ .environment = &environment };
    const loaded = try Desktop.source(&desktop);
    try std.testing.expectEqual(native_sdk.WebViewSourceKind.url, loaded.kind);
    try std.testing.expectEqualStrings("http://127.0.0.1:5175/", loaded.bytes);
}
