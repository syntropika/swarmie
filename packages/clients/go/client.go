// Package mpai provides a bounded HTTP JSON-RPC client for the local runtime.
package mpai

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync/atomic"
	"time"
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
	HTTPClient       *http.Client
}
type Client struct {
	endpoint         string
	token            string
	timeout          time.Duration
	maxResponseBytes int64
	http             *http.Client
	prefix           string
	sequence         atomic.Uint64
}

func New(endpoint string, options Options) (*Client, error) {
	parsed, err := url.Parse(endpoint)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" {
		return nil, errors.New("endpoint must be an HTTP or HTTPS URL")
	}
	if parsed.Path == "" || parsed.Path == "/" {
		parsed.Path = "/rpc"
	}
	if options.Timeout == 0 {
		options.Timeout = 30 * time.Second
	}
	if options.MaxResponseBytes == 0 {
		options.MaxResponseBytes = 16 * 1024 * 1024
	}
	if options.Timeout < 0 || options.MaxResponseBytes < 0 {
		return nil, errors.New("limits must be positive")
	}
	random := make([]byte, 16)
	if _, err = rand.Read(random); err != nil {
		return nil, err
	}
	if options.HTTPClient == nil {
		options.HTTPClient = &http.Client{CheckRedirect: func(_ *http.Request, _ []*http.Request) error { return http.ErrUseLastResponse }}
	}
	return &Client{endpoint: parsed.String(), token: options.Token, timeout: options.Timeout, maxResponseBytes: options.MaxResponseBytes, http: options.HTTPClient, prefix: hex.EncodeToString(random)}, nil
}

// Call invokes any method from the canonical schema. Credentials remain in memory.
func (c *Client) Call(ctx context.Context, method string, params any, result any) error {
	ctx, cancel := context.WithTimeout(ctx, c.timeout)
	defer cancel()
	id := fmt.Sprintf("%s-%d", c.prefix, c.sequence.Add(1))
	payload, err := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
	if err != nil {
		return err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint, bytes.NewReader(payload))
	if err != nil {
		return err
	}
	request.Header.Set("Content-Type", "application/json")
	if c.token != "" {
		request.Header.Set("Authorization", "Bearer "+c.token)
	}
	response, err := c.http.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return &ProtocolError{Message: fmt.Sprintf("HTTP %d", response.StatusCode)}
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, c.maxResponseBytes+1))
	if err != nil {
		return err
	}
	if int64(len(body)) > c.maxResponseBytes {
		return &ProtocolError{Message: "response exceeds size limit"}
	}
	var envelope struct {
		JSONRPC string          `json:"jsonrpc"`
		ID      json.RawMessage `json:"id"`
		Result  json.RawMessage `json:"result"`
		Error   *RPCError       `json:"error"`
	}
	if err = json.Unmarshal(body, &envelope); err != nil {
		return &ProtocolError{Message: "response is not valid JSON"}
	}
	var receivedID string
	if envelope.JSONRPC != "2.0" || json.Unmarshal(envelope.ID, &receivedID) != nil || receivedID != id {
		return &ProtocolError{Message: "invalid JSON-RPC response identity"}
	}
	if envelope.Error != nil {
		return envelope.Error
	}
	if envelope.Result == nil {
		return &ProtocolError{Message: "response has no result"}
	}
	if result == nil {
		return nil
	}
	return json.Unmarshal(envelope.Result, result)
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

// Events polls the durable transcript. Returning false from consume stops cleanly.
func (c *Client) Events(ctx context.Context, sessionID string, options EventOptions, consume func(RuntimeEvent) bool) error {
	if options.Timeout == 0 {
		options.Timeout = 10 * time.Minute
	}
	if options.PollInterval == 0 {
		options.PollInterval = 200 * time.Millisecond
	}
	if options.Timeout < 0 || options.PollInterval < 0 {
		return errors.New("limits must be positive")
	}
	ctx, cancel := context.WithTimeout(ctx, options.Timeout)
	defer cancel()
	sequence := options.AfterSequence
	for {
		var batch []RuntimeEvent
		if err := c.Call(ctx, "session/transcript", map[string]any{"sessionId": sessionID, "afterSequence": sequence, "limit": 256}, &batch); err != nil {
			return err
		}
		for _, event := range batch {
			if event.SessionId != sessionID || event.Sequence <= sequence {
				return &ProtocolError{Message: "transcript is not strictly ordered for this session"}
			}
			sequence = event.Sequence
			if !consume(event) {
				return nil
			}
		}
		if len(batch) >= 256 {
			continue
		}
		timer := time.NewTimer(options.PollInterval)
		select {
		case <-ctx.Done():
			timer.Stop()
			return ctx.Err()
		case <-timer.C:
		}
	}
}
func (c *Client) WaitForTurn(ctx context.Context, sessionID string, options EventOptions) ([]RuntimeEvent, error) {
	var events []RuntimeEvent
	var ended bool
	err := c.Events(ctx, sessionID, options, func(event RuntimeEvent) bool {
		events = append(events, event)
		if event.Type == "session.ended" {
			return false
		}
		if event.Type == "turn.ended" {
			ended = true
			return false
		}
		return true
	})
	if err == nil && !ended {
		err = &ProtocolError{Message: "session ended before the requested turn"}
	}
	return events, err
}

// String returns only the endpoint and deliberately excludes the bearer token.
func (c *Client) String() string { return "MPAIClient(" + strings.Split(c.endpoint, "?")[0] + ")" }
