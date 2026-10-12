const std = @import("std");
const native = @import("native_sdk");
const bridge = native.bridge;

// The WebView can invoke this application command, never arbitrary URLs or SDK services.
pub const ClientBridge = struct {
    io: std.Io,
    origin: []const u8,
    token: ?[]const u8,
    runtime: ?*native.Runtime = null,
    stopping: std.atomic.Value(bool) = .init(false),
    jobs: [32]?*Job = @splat(null),

    const Job = struct {
        owner: *ClientBridge,
        id: []const u8,
        payload: []const u8,
        responder: bridge.AsyncResponder,
        output: []u8,
        length: usize = 0,
        failure: ?anyerror = null,
        done: std.atomic.Value(bool) = .init(false),
        future: ?std.Io.Future(void) = null,
    };

    pub fn start(context: *anyopaque, runtime: *native.Runtime) !void {
        const self: *ClientBridge = @ptrCast(@alignCast(context));
        self.runtime = runtime;
    }
    pub fn event(context: *anyopaque, _: *native.Runtime, event_value: native.Event) !void {
        const self: *ClientBridge = @ptrCast(@alignCast(context));
        if (event_value == .effects_wake) try self.drain();
    }
    pub fn stop(context: *anyopaque, _: *native.Runtime) !void {
        const self: *ClientBridge = @ptrCast(@alignCast(context));
        self.stopping.store(true, .release);
        for (&self.jobs) |*slot| {
            if (slot.*) |job| {
                if (job.future) |*future| future.cancel(self.io);
                destroy(job);
                slot.* = null;
            }
        }
        self.runtime = null;
    }
    pub fn invoke(context: *anyopaque, invocation: bridge.Invocation, responder: bridge.AsyncResponder) !void {
        const self: *ClientBridge = @ptrCast(@alignCast(context));
        if (self.token == null or !std.mem.eql(u8, invocation.source.origin, self.origin) or !std.mem.eql(u8, invocation.source.webview_label, "main") or invocation.source.window_id != 1) {
            return responder.fail(invocation.request.id, .permission_denied, "Native client command is unavailable for this view");
        }
        try self.drain();
        for (&self.jobs) |*slot| {
            if (slot.* != null) continue;
            const allocator = std.heap.page_allocator;
            const job = try allocator.create(Job);
            errdefer allocator.destroy(job);
            const id = try allocator.dupe(u8, invocation.request.id);
            errdefer allocator.free(id);
            const payload = try allocator.dupe(u8, invocation.request.payload);
            errdefer allocator.free(payload);
            const output = try allocator.alloc(u8, bridge.max_result_bytes);
            errdefer allocator.free(output);
            job.* = .{ .owner = self, .id = id, .payload = payload, .responder = responder, .output = output };
            job.future = try std.Io.concurrent(self.io, work, .{job});
            slot.* = job;
            return;
        }
        try responder.fail(invocation.request.id, .handler_failed, "Native client request capacity exceeded before dispatch");
    }
    fn work(job: *Job) void {
        perform(job) catch |err| {
            job.failure = err;
        };
        job.done.store(true, .release);
        if (!job.owner.stopping.load(.acquire)) {
            if (job.owner.runtime) |runtime| runtime.options.platform.services.wake() catch {};
        }
    }
    fn perform(job: *Job) !void {
        const allocator = std.heap.page_allocator;
        var client: std.http.Client = .{ .allocator = allocator, .io = job.owner.io };
        defer client.deinit();
        const url = try std.fmt.allocPrint(allocator, "{s}/ipc", .{job.owner.origin});
        defer allocator.free(url);
        const auth = try std.fmt.allocPrint(allocator, "Bearer {s}", .{job.owner.token.?});
        defer allocator.free(auth);
        var request = try client.request(.POST, try std.Uri.parse(url), .{
            .redirect_behavior = .unhandled,
            .keep_alive = false,
            .headers = .{ .content_type = .{ .override = "application/json" }, .authorization = .{ .override = auth } },
        });
        defer request.deinit();
        request.transfer_encoding = .{ .content_length = job.payload.len };
        var body = try request.sendBodyUnflushed(&.{});
        try body.writer.writeAll(job.payload);
        try body.end();
        try request.connection.?.flush();
        var response = try request.receiveHead(&.{});
        const status = response.head.status;
        var buffer: [8192]u8 = undefined;
        const reader = response.reader(&buffer);
        var writer = std.Io.Writer.fixed(job.output);
        _ = try reader.streamRemaining(&writer);
        job.length = writer.end;
        if (status != .ok) return error.NativeClientRequestFailed;
    }
    fn drain(self: *ClientBridge) !void {
        for (&self.jobs) |*slot| {
            const job = slot.* orelse continue;
            if (!job.done.load(.acquire)) continue;
            if (job.future) |*future| future.await(self.io);
            slot.* = null;
            defer destroy(job);
            if (job.failure) |failure| {
                try job.responder.fail(job.id, .handler_failed, if (failure == error.NativeClientRequestFailed and job.length > 0) job.output[0..job.length] else @errorName(failure));
            } else {
                try job.responder.success(job.id, job.output[0..job.length]);
            }
        }
    }
    fn destroy(job: *Job) void {
        const allocator = std.heap.page_allocator;
        allocator.free(job.id);
        allocator.free(job.payload);
        allocator.free(job.output);
        allocator.destroy(job);
    }
};
