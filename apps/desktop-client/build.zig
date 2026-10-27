const std = @import("std");
const native = @import("native_sdk");

pub fn build(b: *std.Build) void {
    const app = native.addAppArtifacts(b, b.dependency("native_sdk", .{}), .{
        .name = "swarmie",
        .manifest = "app.json",
    });
    const windows = b.graph.host.result.os.tag == .windows;
    const archive = b.path(if (windows) "../../target/release/swarmie_desktop_updates.lib" else "../../target/release/libswarmie_desktop_updates.a");
    for ([_]*std.Build.Step.Compile{ app.exe, app.tests }) |artifact| {
        artifact.root_module.addObjectFile(archive);
        if (windows) {
            for ([_][]const u8{ "advapi32", "bcrypt", "crypt32", "ncrypt", "ntdll", "ole32", "secur32", "userenv", "ws2_32", "synchronization" }) |name| artifact.root_module.linkSystemLibrary(name, .{});
        } else if (b.graph.host.result.os.tag == .linux) {
            for ([_][]const u8{ "gcc_s", "util", "rt", "pthread", "m", "dl" }) |name| artifact.root_module.linkSystemLibrary(name, .{});
        }
    }
}
