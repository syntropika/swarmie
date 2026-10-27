const std = @import("std");
const native = @import("native_sdk");

pub fn build(b: *std.Build) void {
    const app = native.addAppArtifacts(b, b.dependency("native_sdk", .{}), .{
        .name = "swarmie",
        .manifest = "app.json",
    });
    const windows = b.graph.host.result.os.tag == .windows;
    const winrt = if (windows) path: {
        const sdk = std.zig.WindowsSdk.find(b.allocator, b.graph.io, b.graph.host.result.cpu.arch, &b.graph.environ_map) catch @panic("A Windows SDK with WinRT headers is required");
        defer sdk.free(b.allocator);
        const installed = sdk.windows10sdk orelse @panic("A Windows SDK with WinRT headers is required");
        break :path b.pathJoin(&.{ installed.path, "Include", installed.version, "winrt" });
    } else null;
    const archive = b.path(if (windows) "../../target/release/swarmie_desktop_updates.lib" else "../../target/release/libswarmie_desktop_updates.a");
    for ([_]*std.Build.Step.Compile{ app.exe, app.tests }) |artifact| {
        // The SDK reuses the app module for semantic analysis and contract emission.
        // Attach Rust only to the final executable and test runner, not those tools.
        const linked = b.allocator.create(std.Build.Module) catch @panic("OOM");
        linked.init(b, .{ .existing = artifact.root_module });
        linked.link_objects = linked.link_objects.clone(b.allocator) catch @panic("OOM");
        linked.c_macros = linked.c_macros.clone(b.allocator) catch @panic("OOM");
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
            artifact.root_module.link_libcpp = false;
            artifact.root_module.addCMacro("NOMINMAX", "1");
            artifact.root_module.addSystemIncludePath(.{ .cwd_relative = winrt.? });
            for ([_][]const u8{ "advapi32", "bcrypt", "crypt32", "ncrypt", "ntdll", "ole32", "secur32", "userenv", "ws2_32", "synchronization", "msvcprt", "uuid" }) |name| artifact.root_module.linkSystemLibrary(name, .{});
        } else if (b.graph.host.result.os.tag == .linux) {
            for ([_][]const u8{ "gcc_s", "util", "rt", "pthread", "m", "dl" }) |name| artifact.root_module.linkSystemLibrary(name, .{});
        }
    }
    if (windows) {
        app.exe.root_module.addCSourceFile(.{ .file = b.path("src/windows_guids.cpp"), .flags = &.{"-std=c++17"} });
        // Keep the GUI subsystem while the CRT initializes and calls Zig's C main.
        app.exe.entry = .{ .symbol_name = "mainCRTStartup" };
    }
}
