package mpai

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"
)

func TestHTTPBoundsAndIdentity(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request map[string]any
		_ = json.NewDecoder(r.Body).Decode(&request)
		id := request["id"]
		result := any(nil)
		if request["method"] == "wrong" {
			id = "wrong"
		}
		if request["method"] == "large" {
			result = make([]int, 2048)
		}
		if request["method"] == "slow" {
			time.Sleep(60 * time.Millisecond)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"jsonrpc": "2.0", "id": id, "result": result})
	}))
	defer server.Close()
	client, err := New(server.URL, Options{Timeout: 20 * time.Millisecond, MaxResponseBytes: 1024})
	if err != nil {
		t.Fatal(err)
	}
	if err = client.Call(context.Background(), "null", map[string]any{}, nil); err != nil {
		t.Fatal(err)
	}
	for _, method := range []string{"wrong", "large", "slow"} {
		if client.Call(context.Background(), method, map[string]any{}, nil) == nil {
			t.Fatalf("%s unexpectedly succeeded", method)
		}
	}
}
func TestPollingDeadline(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request map[string]any
		_ = json.NewDecoder(r.Body).Decode(&request)
		_ = json.NewEncoder(w).Encode(map[string]any{"jsonrpc": "2.0", "id": request["id"], "result": []any{}})
	}))
	defer server.Close()
	client, _ := New(server.URL, Options{})
	_, err := client.WaitForTurn(context.Background(), "s", EventOptions{Timeout: 20 * time.Millisecond, PollInterval: 2 * time.Millisecond})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected deadline, got %v", err)
	}
}
func TestLiveHTTPConformance(t *testing.T) {
	endpoint := os.Getenv("MPAI_TEST_URL")
	if endpoint == "" {
		t.Skip("live daemon URL required")
	}
	ctx := context.Background()
	client, err := New(endpoint, Options{Token: os.Getenv("MPAI_TEST_TOKEN")})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = client.Initialize(ctx); err != nil {
		t.Fatal(err)
	}
	schema, err := client.Schema(ctx)
	if err != nil {
		t.Fatal(err)
	}
	var canonical map[string]json.RawMessage
	if json.Unmarshal(schema, &canonical) != nil || canonical["x-methods"] == nil {
		t.Fatal("missing canonical schema")
	}
	harnesses, err := client.Discover(ctx)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, h := range harnesses {
		found = found || h.Harness == "mock" && h.Available
	}
	if !found {
		t.Fatal("mock harness is absent")
	}
	err = client.Call(ctx, "not/a/method", map[string]any{}, nil)
	var rpcError *RPCError
	if !errors.As(err, &rpcError) || rpcError.Code != -32601 {
		t.Fatalf("expected structured error, got %v", err)
	}
	cwd := os.Getenv("MPAI_TEST_CWD")
	if cwd == "" {
		cwd, _ = os.Getwd()
	}
	session, err := client.Spawn(ctx, InputSpawnOptions{Harness: "mock", Cwd: &cwd})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(ctx, session.Id)
	if session.Capabilities.Queue.Source != "runtime" || session.Capabilities.Configure.Availability != "available" {
		t.Fatal("capability contract")
	}
	var models []ModelDescriptor
	if err = client.Call(ctx, "agent/models", map[string]any{"sessionId": session.Id}, &models); err != nil || len(models) != 1 || string(models[0].Metadata["nativeField"]) != `"retained"` {
		t.Fatal("model catalog", err)
	}
	var settings []SettingDescriptor
	if err = client.Call(ctx, "agent/configuration", map[string]any{"sessionId": session.Id}, &settings); err != nil || len(settings) != 2 || settings[0].Application != "next_turn" || settings[1].Id != "executionProfile" {
		t.Fatal("configuration catalog", err)
	}
	var configured ConfigurationReceipt
	if err = client.Call(ctx, "agent/configure", map[string]any{"sessionId": session.Id, "config": map[string]any{"fast": true}}, &configured); err != nil || string(configured.Values["fast"]) != "true" {
		t.Fatal("configuration receipt", err)
	}
	if configured.ExecutionPolicy.Status != "unknown" || configured.ExecutionPolicy.DispatchBlocked || len(configured.ExecutionPolicy.Choices) != 3 {
		t.Fatal("execution policy receipt")
	}
	receipt, err := client.PromptContent(ctx, session.Id, []InputPromptContent{TextContent("Reply")}, "request-1")
	if err != nil || receipt.Delivery != "accepted" {
		t.Fatalf("prompt: %+v %v", receipt, err)
	}
	events, err := client.WaitForTurn(ctx, session.Id, EventOptions{AfterSequence: session.Sequence, Timeout: 3 * time.Second, PollInterval: 5 * time.Millisecond})
	if err != nil {
		t.Fatal(err)
	}
	text := ""
	var last uint64
	for _, e := range events {
		if e.Sequence <= last {
			t.Fatal("event order")
		}
		last = e.Sequence
		if e.Type == "text.delta" {
			var data struct {
				Text string `json:"text"`
			}
			if err = json.Unmarshal(e.Data, &data); err != nil {
				t.Fatal(err)
			}
			text += data.Text
		}
	}
	if text != "client-ready" || events[len(events)-1].Type != "turn.ended" {
		t.Fatalf("unexpected output: %q", text)
	}
	var terminal struct {
		Outcome TurnOutcome `json:"outcome"`
	}
	if err = json.Unmarshal(events[len(events)-1].Data, &terminal); err != nil || terminal.Outcome != "completed" {
		t.Fatalf("unexpected terminal outcome: %+v %v", terminal, err)
	}
	recovered, err := client.Submission(ctx, session.Id, "request-1")
	if err != nil || recovered == nil || recovered.Id != receipt.Id {
		t.Fatal("submission recovery", err)
	}
	reconciled, err := client.ReconcileSubmission(ctx, session.Id, "request-1")
	if err != nil || reconciled == nil || reconciled.Id != recovered.Id {
		t.Fatal("reconciliation receipt", err)
	}

	duplicate, err := client.Prompt(ctx, session.Id, "Reply", "request-1")
	if err != nil || duplicate.Id != receipt.Id {
		t.Fatal("duplicate submission", err)
	}
	_, err = client.Prompt(ctx, session.Id, "Changed", "request-1")
	var conflict *RPCError
	if !errors.As(err, &conflict) || conflict.Code != -32004 {
		t.Fatal("idempotency conflict", err)
	}
	var replay []RuntimeEvent
	if err = client.Call(ctx, "session/transcript", map[string]any{"sessionId": session.Id, "limit": 4096}, &replay); err != nil || len(replay) < len(events) {
		t.Fatal("durable replay failed", err)
	}
	denied, _ := New(endpoint, Options{Token: "incorrect"})
	if _, err = denied.Initialize(ctx); err == nil {
		t.Fatal("invalid auth accepted")
	}
}
