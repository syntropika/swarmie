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
        // The SDK reuses the app module for semantic analysis and contract emission.
        // Attach Rust only to the final executable and test runner, not those tools.
        const linked = b.allocator.create(std.Build.Module) catch @panic("OOM");
        linked.init(b, .{ .existing = artifact.root_module });
        linked.link_objects = linked.link_objects.clone(b.allocator) catch @panic("OOM");
        linked.cached_graph = .{ .modules = &.{}, .names = &.{} };
        artifact.root_module = linked;
        artifact.root_module.addObjectFile(archive);
        if (windows) {
            // Use Rust's builtins and the MSVC CRT. Zig's bundled COFF runtimes
            // conflict with those builtins, including unused UBSan float helpers.
            artifact.bundle_compiler_rt = false;
            artifact.bundle_ubsan_rt = false;
            artifact.linkage = .dynamic;
            artifact.root_module.link_libc = true;
            for ([_][]const u8{ "advapi32", "bcrypt", "crypt32", "ncrypt", "ntdll", "ole32", "secur32", "userenv", "ws2_32", "synchronization" }) |name| artifact.root_module.linkSystemLibrary(name, .{});
        } else if (b.graph.host.result.os.tag == .linux) {
            for ([_][]const u8{ "gcc_s", "util", "rt", "pthread", "m", "dl" }) |name| artifact.root_module.linkSystemLibrary(name, .{});
        }
    }
}
