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
	session, err := client.Spawn(ctx, InputSpawnOptions{Harness: "mock", Cwd: cwd})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(ctx, session.Id)
	receipt, err := client.PromptContent(ctx, session.Id, []InputPromptContent{TextContent("Reply")})
	if err != nil || !receipt.Accepted {
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
	var replay []RuntimeEvent
	if err = client.Call(ctx, "session/transcript", map[string]any{"sessionId": session.Id, "limit": 4096}, &replay); err != nil || len(replay) < len(events) {
		t.Fatal("durable replay failed", err)
	}
	denied, _ := New(endpoint, Options{Token: "incorrect"})
	if _, err = denied.Initialize(ctx); err == nil {
		t.Fatal("invalid auth accepted")
	}
}
