const std = @import("std");
const runner = @import("runner");
const native = @import("native_sdk");
const builtin = @import("builtin");
pub const panic = std.debug.FullPanic(native.debug.capturePanic);
const default_url = "http://127.0.0.1:7447";

fn localOrigin(url: []const u8) ![]const u8 {
    const uri = try std.Uri.parse(url);
    if (!std.mem.eql(u8, uri.scheme, "http") or uri.user != null or uri.password != null) return error.InvalidDesktopUrl;
    const host = uri.host orelse return error.InvalidDesktopUrl;
    if (!std.mem.eql(u8, host.percent_encoded, "127.0.0.1") and !std.mem.eql(u8, host.percent_encoded, "[::1]")) return error.InvalidDesktopUrl;
    const end = std.mem.indexOfScalarPos(u8, url, 7, '/') orelse url.len;
    return url[0..end];
}

pub fn main(init: std.process.Init) !void {
    if (init.environ_map.get("MPAI_DESKTOP_MANAGED") == null and init.environ_map.get("MPAI_DESKTOP_URL") == null) {
        if (try launchPackaged(init)) return;
    }
    var context: u8 = 0;
    const url = init.environ_map.get("MPAI_DESKTOP_URL") orelse default_url;
    const origin = try localOrigin(url);
    const origins = [_][]const u8{origin};
    try runner.runWithOptions(.{ .context = &context, .name = "mpai-desktop", .source = native.WebViewSource.url(url) }, .{
        .app_name = "mpai-desktop",
        .window_title = "mpai",
        .bundle_id = "dev.mpai.desktop",
        .icon_path = "assets/icon.png",
        .default_frame = native.geometry.RectF.init(0, 0, 1440, 900),
        // Embedded project and extension content must never reach privileged SDK commands.
        .builtin_bridge = .{ .enabled = false },
        .security = .{ .navigation = .{ .allowed_origins = &origins } },
    }, init);
}

fn packagePaths(allocator: std.mem.Allocator, executable: []const u8) !struct { service: []const u8, assets: []const u8 } {
    const directory = std.fs.path.dirname(executable) orelse return error.InvalidPackagePath;
    const name = std.fs.path.basename(executable);
    return .{
        .service = try std.fmt.allocPrint(allocator, "{s}/{s}_services", .{ directory, name }),
        .assets = try std.fs.path.join(allocator, &.{ directory, "..", if (builtin.os.tag == .macos) "Resources" else "resources" }),
    };
}

fn launchPackaged(init: std.process.Init) !bool {
    var buffer: [std.Io.Dir.max_path_bytes]u8 = undefined;
    const length = try std.process.executablePath(init.io, &buffer);
    const executable = buffer[0..length];
    const paths = try packagePaths(init.gpa, executable);
    defer init.gpa.free(paths.service);
    defer init.gpa.free(paths.assets);
    std.Io.Dir.cwd().access(init.io, paths.service, .{}) catch |err| switch (err) {
        error.FileNotFound => return false,
        else => return err,
    };
    var child = try std.process.spawn(init.io, .{
        .argv = &.{ paths.service, "desktop", "--assets", paths.assets, "--host", executable },
        .environ_map = init.environ_map,
        .stdin = .inherit,
        .stdout = .inherit,
        .stderr = .inherit,
    });
    const result = try child.wait(init.io);
    switch (result) {
        .exited => |code| if (code != 0) return error.DesktopLaunchFailed,
        else => return error.DesktopLaunchFailed,
    }
    return true;
}

test "packaged host resolves its own daemon and assets without a working-directory dependency" {
    const paths = try packagePaths(std.testing.allocator, "/opt/mpai/bin/mpai-desktop");
    defer std.testing.allocator.free(paths.service);
    defer std.testing.allocator.free(paths.assets);
    try std.testing.expectEqualStrings("/opt/mpai/bin/mpai-desktop_services", paths.service);
    try std.testing.expect(std.mem.endsWith(u8, paths.assets, if (builtin.os.tag == .macos) "/Resources" else "/resources"));
}

test "desktop navigation is restricted to a local HTTP origin" {
    try std.testing.expectEqualStrings(default_url, try localOrigin(default_url));
    try std.testing.expectEqualStrings("http://127.0.0.1:8090", try localOrigin("http://127.0.0.1:8090/"));
    try std.testing.expectError(error.InvalidDesktopUrl, localOrigin("https://example.com"));
    try std.testing.expectError(error.InvalidDesktopUrl, localOrigin("http://127.0.0.1.evil.test:7447"));
}
