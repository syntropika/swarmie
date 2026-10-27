const std = @import("std");
const native = @import("native_sdk");

const Handle = opaque {};
const Snapshot = extern struct {
    phase: u32 = 0,
    version: [96]u8 = @splat(0),
    failure: [512]u8 = @splat(0),
};

extern fn swarmie_updater_start() ?*Handle;
extern fn swarmie_updater_snapshot(*const Handle, *Snapshot) void;
extern fn swarmie_updater_install(*const Handle) bool;
extern fn swarmie_updater_close(*Handle) void;

const timer_id = 0x737761726d696575;

/// The native host owns the worker and shows installation consent on the UI thread.
pub const Updates = struct {
    handle: ?*Handle = null,
    offered: bool = false,
    installing: bool = false,

    pub fn init(development: bool) Updates {
        return .{ .handle = if (development) null else swarmie_updater_start() };
    }

    pub fn start(self: *Updates, runtime: *native.Runtime) !void {
        if (self.handle != null) try runtime.startTimer(timer_id, std.time.ns_per_s, true);
    }

    pub fn event(self: *Updates, runtime: *native.Runtime, event_value: native.Event) !void {
        if (event_value != .timer or event_value.timer.id != timer_id) return;
        const handle = self.handle orelse return;
        var snapshot = Snapshot{};
        swarmie_updater_snapshot(handle, &snapshot);
        switch (snapshot.phase) {
            3 => {
                if (self.offered) return;
                self.offered = true;
                var message: [192]u8 = undefined;
                const version = std.mem.sliceTo(&snapshot.version, 0);
                const choice = try runtime.showMessageDialog(.{
                    .title = "Swarmie update",
                    .message = try std.fmt.bufPrint(&message, "Swarmie {s} is available.", .{version}),
                    .informative_text = "Update downloads and installs the new version, then restarts Swarmie. Save your work before continuing.",
                    .primary_button = "Update",
                    .secondary_button = "Later",
                });
                if (choice == .primary) self.installing = swarmie_updater_install(handle);
            },
            5 => try runtime.quitApp(),
            6 => {
                try runtime.cancelTimer(timer_id);
                if (self.installing) {
                    _ = try runtime.showMessageDialog(.{
                        .style = .warning,
                        .title = "Update could not finish",
                        .message = "Swarmie will keep running with the current version.",
                        .informative_text = std.mem.sliceTo(&snapshot.failure, 0),
                    });
                } else std.log.warn("update check failed: {s}", .{std.mem.sliceTo(&snapshot.failure, 0)});
            },
            2 => try runtime.cancelTimer(timer_id),
            else => {},
        }
    }

    pub fn deinit(self: *Updates) void {
        if (self.handle) |handle| swarmie_updater_close(handle);
        self.handle = null;
    }
};
