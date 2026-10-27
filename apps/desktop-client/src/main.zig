const std = @import("std");
const native_sdk = @import("native_sdk");
const runner = @import("runner");

pub const panic = std.debug.FullPanic(native_sdk.debug.capturePanic);

const frontend: native_sdk.frontend.Config = .{ .dist = "dist" };
const origins = [_][]const u8{ "zero://app", "http://127.0.0.1:5175" };

/// The desktop host loads the existing client; its plugins continue to own presentation state.
const Desktop = struct {
    environment: *std.process.Environ.Map,

    fn app(self: *Desktop) native_sdk.App {
        return .{
            .context = self,
            .name = "swarmie",
            .source = native_sdk.frontend.productionSource(frontend),
            .source_fn = source,
        };
    }

    fn source(context: *anyopaque) anyerror!native_sdk.WebViewSource {
        const self: *Desktop = @ptrCast(@alignCast(context));
        return native_sdk.frontend.sourceFromEnv(self.environment, frontend);
    }
};

pub fn main(init: std.process.Init) !void {
    var desktop = Desktop{ .environment = init.environ_map };
    try runner.runWithOptions(desktop.app(), .{
        .app_name = "swarmie",
        .window_title = "Swarmie",
        .bundle_id = "app.swarmie.desktop",
        .icon_path = "assets/icon.png",
        .default_frame = native_sdk.geometry.RectF.init(0, 0, 1280, 800),
        .security = .{ .navigation = .{ .allowed_origins = &origins } },
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
