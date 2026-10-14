// Package swarmie marshals canonical wire types through the native Rust client.
package swarmie

/*
#cgo linux LDFLAGS: ${SRCDIR}/native/libswarmie_client_ffi.a -ldl -lpthread -lm
#cgo darwin LDFLAGS: ${SRCDIR}/native/libswarmie_client_ffi.a -framework Security -framework SystemConfiguration
#include <stdlib.h>
#include "native/swarmie_client.h"
*/
import "C"

import (
	"context"
	"encoding/json"
	"fmt"
	"runtime"
	"time"
	"unsafe"
)

type RPCError struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}

func (e *RPCError) Error() string { return fmt.Sprintf("RPC %d: %s", e.Code, e.Message) }

type ProtocolError struct{ Message string }

func (e *ProtocolError) Error() string { return e.Message }

type Options struct {
	Token            string
	Timeout          time.Duration
	MaxResponseBytes int64
}
type Client struct{ handle uint64 }

func native(command any) (json.RawMessage, error) {
	payload, err := json.Marshal(command)
	if err != nil {
		return nil, err
	}
	input := C.CString(string(payload))
	defer C.free(unsafe.Pointer(input))
	output := C.swarmie_client_execute(input)
	if output == nil {
		return nil, &ProtocolError{"Native allocation failed"}
	}
	defer C.swarmie_client_free(output)
	var reply struct {
		OK    json.RawMessage `json:"ok"`
		Error *struct {
			Kind    string  `json:"kind"`
			Message string  `json:"message"`
			Code    int     `json:"code"`
			Data    *string `json:"data_json"`
		} `json:"error"`
	}
	if err = json.Unmarshal([]byte(C.GoString(output)), &reply); err != nil {
		return nil, err
	}
	if reply.Error == nil {
		return reply.OK, nil
	}
	e := reply.Error
	switch e.Kind {
	case "rpc":
		var data json.RawMessage
		if e.Data != nil {
			data = json.RawMessage(*e.Data)
		}
		return nil, &RPCError{e.Code, e.Message, data}
	case "timeout":
		return nil, context.DeadlineExceeded
	case "cancelled":
		return nil, context.Canceled
	default:
		return nil, &ProtocolError{e.Message}
	}
}
func create(command any) (*Client, error) {
	if C.swarmie_client_abi_version() != 1 {
		return nil, &ProtocolError{"Unsupported native client ABI"}
	}
	value, err := native(command)
	if err != nil {
		return nil, err
	}
	c := &Client{}
	if err = json.Unmarshal(value, &c.handle); err != nil {
		return nil, err
	}
	runtime.SetFinalizer(c, func(client *Client) { _ = client.Dispose() })
	return c, nil
}
func defaults(options Options) Options {
	if options.Timeout == 0 {
		options.Timeout = 30 * time.Second
	}
	if options.MaxResponseBytes == 0 {
		options.MaxResponseBytes = 16 * 1024 * 1024
	}
	return options
}
func New(endpoint string, options Options) (*Client, error) {
	options = defaults(options)
	return create(map[string]any{"action": "create_http", "config": map[string]any{"url": endpoint, "token": options.Token, "timeout_ms": options.Timeout.Milliseconds(), "max_response_bytes": options.MaxResponseBytes}})
}

// NewStdio owns a runtime child until Dispose or a request deadline/cancellation.
func NewStdio(command string, args []string, options Options) (*Client, error) {
	options = defaults(options)
	if args == nil {
		args = []string{}
	}
	return create(map[string]any{"action": "create_stdio", "config": map[string]any{"command": command, "args": args, "timeout_ms": options.Timeout.Milliseconds(), "max_response_bytes": options.MaxResponseBytes}})
}

// Dispose closes native resources. In-flight calls finish with a closed error.
func (c *Client) Dispose() error {
	_, err := native(map[string]any{"action": "release", "handle": c.handle})
	runtime.KeepAlive(c)
	return err
}
func operation(ctx context.Context, command map[string]any, result any) error {
	prepared, err := native(map[string]any{"action": "prepare"})
	if err != nil {
		return err
	}
	var id uint64
	if err = json.Unmarshal(prepared, &id); err != nil {
		return err
	}
	defer func() { _, _ = native(map[string]any{"action": "release", "handle": id}) }()
	done := make(chan struct{})
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		select {
		case <-ctx.Done():
			_, _ = native(map[string]any{"action": "cancel", "operation": id})
		case <-done:
		}
	}()
	// Cancellation must precede dispatch when the context was already cancelled.
	if ctx.Err() != nil {
		_, _ = native(map[string]any{"action": "cancel", "operation": id})
	}
	command["operation"] = id
	value, err := native(command)
	close(done)
	<-finished
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if err != nil {
		return err
	}
	var encoded string
	if err = json.Unmarshal(value, &encoded); err != nil {
		return err
	}
	if result == nil {
		return nil
	}
	return json.Unmarshal([]byte(encoded), result)
}

// Call executes a bounded native RPC request. No automatic retries are performed.
func (c *Client) Call(ctx context.Context, method string, params any, result any) error {
	encoded, err := json.Marshal(params)
	if err != nil {
		return err
	}
	err = operation(ctx, map[string]any{"action": "invoke", "client": c.handle, "request": map[string]any{"method": method, "params_json": string(encoded), "timeout_ms": nil}}, result)
	runtime.KeepAlive(c)
	return err
}
func (c *Client) Initialize(ctx context.Context) (json.RawMessage, error) {
	var value json.RawMessage
	err := c.Call(ctx, "runtime/initialize", map[string]any{}, &value)
	return value, err
}
func (c *Client) Schema(ctx context.Context) (json.RawMessage, error) {
	var value json.RawMessage
	err := c.Call(ctx, "runtime/schema", map[string]any{}, &value)
	return value, err
}
func (c *Client) Discover(ctx context.Context) ([]Detection, error) {
	var value []Detection
	err := c.Call(ctx, "runtime/discover", map[string]any{}, &value)
	return value, err
}
func (c *Client) Spawn(ctx context.Context, options InputSpawnOptions) (SessionSnapshot, error) {
	var value SessionSnapshot
	err := c.Call(ctx, "session/create", options, &value)
	return value, err
}

func (c *Client) Prompt(ctx context.Context, sessionID, text, idempotencyKey string) (SubmissionReceipt, error) {
	var value SubmissionReceipt
	err := c.Call(ctx, "session/prompt", map[string]any{"sessionId": sessionID, "text": text, "idempotencyKey": idempotencyKey}, &value)
	return value, err
}
func (c *Client) PromptContent(ctx context.Context, sessionID string, content []InputPromptContent, idempotencyKey string) (SubmissionReceipt, error) {
	var value SubmissionReceipt
	err := c.Call(ctx, "session/prompt", map[string]any{"sessionId": sessionID, "content": content, "idempotencyKey": idempotencyKey}, &value)
	return value, err
}
func (c *Client) Submission(ctx context.Context, sessionID, idempotencyKey string) (*SubmissionReceipt, error) {
	var value *SubmissionReceipt
	err := c.Call(ctx, "session/submission", map[string]any{"sessionId": sessionID, "idempotencyKey": idempotencyKey}, &value)
	return value, err
}

// ReconcileSubmission inspects an open native connection without submitting work.
func (c *Client) ReconcileSubmission(ctx context.Context, sessionID, idempotencyKey string) (*SubmissionReceipt, error) {
	var value *SubmissionReceipt
	err := c.Call(ctx, "session/submission/reconcile", map[string]any{"sessionId": sessionID, "idempotencyKey": idempotencyKey}, &value)
	return value, err
}
func (c *Client) Close(ctx context.Context, sessionID string) error {
	return c.Call(ctx, "session/close", map[string]any{"sessionId": sessionID}, nil)
}
func TextContent(text string) InputPromptContent {
	value, _ := json.Marshal(map[string]any{"type": "text", "data": text})
	return value
}
func ImageContent(mimeType, base64 string) InputPromptContent {
	value, _ := json.Marshal(map[string]any{"type": "image", "data": map[string]string{"mime_type": mimeType, "data": base64}})
	return value
}

type EventOptions struct {
	AfterSequence uint64
	Timeout       time.Duration
	PollInterval  time.Duration
}

// Events pulls one ordered native event at a time; returning false releases the cursor.
func (c *Client) Events(ctx context.Context, sessionID string, options EventOptions, consume func(RuntimeEvent) bool) error {
	if options.Timeout == 0 {
		options.Timeout = 10 * time.Minute
	}
	if options.PollInterval == 0 {
		options.PollInterval = 200 * time.Millisecond
	}
	value, err := native(map[string]any{"action": "open_events", "client": c.handle, "session_id": sessionID, "options": map[string]any{"after_sequence": options.AfterSequence, "timeout_ms": options.Timeout.Milliseconds(), "poll_interval_ms": options.PollInterval.Milliseconds()}})
	if err != nil {
		return err
	}
	var stream uint64
	if err = json.Unmarshal(value, &stream); err != nil {
		return err
	}
	defer func() { _, _ = native(map[string]any{"action": "release", "handle": stream}); runtime.KeepAlive(c) }()
	for {
		var event RuntimeEvent
		if err = operation(ctx, map[string]any{"action": "next_event", "stream": stream}, &event); err != nil {
			return err
		}
		if !consume(event) {
			return nil
		}
	}
}
func (c *Client) WaitForTurn(ctx context.Context, sessionID string, options EventOptions) ([]RuntimeEvent, error) {
	if options.Timeout == 0 {
		options.Timeout = 10 * time.Minute
	}
	if options.PollInterval == 0 {
		options.PollInterval = 200 * time.Millisecond
	}
	var events []RuntimeEvent
	err := operation(ctx, map[string]any{"action": "wait_turn", "client": c.handle, "session_id": sessionID, "options": map[string]any{"after_sequence": options.AfterSequence, "timeout_ms": options.Timeout.Milliseconds(), "poll_interval_ms": options.PollInterval.Milliseconds()}}, &events)
	runtime.KeepAlive(c)
	return events, err
}

// String never includes credentials or transport parameters.
func (c *Client) String() string { return "SwarmieClient(native)" }
