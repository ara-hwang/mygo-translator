package core

import (
	"context"
	"fmt"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestParseEndpointForms(t *testing.T) {
	e, err := ParseEndpoint("http://127.0.0.1:8788/health")
	if err != nil || e.Host != "127.0.0.1" || e.Port != 8788 {
		t.Fatalf("e = %+v err = %v", e, err)
	}
	e, err = ParseEndpoint("localhost:9000")
	if err != nil || e.Host != "localhost" || e.Port != 9000 {
		t.Fatalf("e = %+v err = %v", e, err)
	}
	e, err = ParseEndpoint("localhost")
	if err != nil || e.Host != "localhost" || e.Port != 8788 {
		t.Fatalf("default port wrong: %+v", e)
	}
	for _, bad := range []string{"", "host:notaport"} {
		if _, err := ParseEndpoint(bad); err == nil {
			t.Fatalf("accepted %q", bad)
		}
	}
	if got := LocalhostEndpoint(8080).Resolved().BaseURL(); got != "http://127.0.0.1:8080" {
		t.Fatalf("base = %s", got)
	}
}

func TestHealthReportsBusyWhileModelLoads(t *testing.T) {
	server := sequenceServer(
		statusStep(503, "Loading model"),
		jsonStep(`{"status":"ok"}`),
	)
	defer server.Close()
	endpoint := mustParse(t, server.URL)
	first, err := ProbeHealth(ctxOf(t), endpoint, HealthTimeout)
	if err != nil || first.Ready {
		t.Fatalf("first = %+v err = %v", first, err)
	}
	second, err := ProbeHealth(ctxOf(t), endpoint, HealthTimeout)
	if err != nil || !second.Ready {
		t.Fatalf("second = %+v err = %v", second, err)
	}
}

func TestStreamsDeltasAndReportsUsage(t *testing.T) {
	server := sequenceServer(sseStep(
		fmt.Sprintf(`{"choices":[{"index":0,"delta":{"content":"%s"}}]}`, "안녕"),
		fmt.Sprintf(`{"choices":[{"index":0,"delta":{"content":"%s"}}]}`, "하세요"),
		`{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":11,"completion_tokens":2},"timings":{"predicted_per_second":42.5}}`,
	))
	defer server.Close()
	endpoint := mustParse(t, server.URL)
	var deltas []string
	result, err := TranslateStream(ctxOf(t), endpoint, "hymt", DefaultGenParams(), "Translate",
		func(delta string) { deltas = append(deltas, delta) })
	if err != nil {
		t.Fatal(err)
	}
	if len(deltas) != 2 || deltas[0] != "안녕" || deltas[1] != "하세요" {
		t.Fatalf("deltas = %v", deltas)
	}
	if result.Text != "안녕하세요" {
		t.Fatalf("text = %q", result.Text)
	}
	if result.PromptTokens != 11 || result.CompletionTokens != 2 {
		t.Fatalf("usage wrong: %+v", result)
	}
	if result.TokensPerSecond != 42.5 {
		t.Fatalf("tokens per second = %v", result.TokensPerSecond)
	}
	if result.FinishReason != "stop" {
		t.Fatalf("finish reason = %q", result.FinishReason)
	}
	if result.FirstTokenMs == 0 || result.Cancelled {
		t.Fatalf("timing = %d cancelled = %v", result.FirstTokenMs, result.Cancelled)
	}
}

func TestSurfacesServerSideErrorsFromAStream(t *testing.T) {
	server := sequenceServer(sseStep(`{"error":{"message":"failed to load model"}}`))
	defer server.Close()
	endpoint := mustParse(t, server.URL)
	_, err := TranslateStream(ctxOf(t), endpoint, "hymt", DefaultGenParams(), "Translate", nil)
	if err == nil || !strings.Contains(err.Error(), "failed to load model") {
		t.Fatalf("err = %v", err)
	}
}

func TestSurfacesHTTPErrorsWithACompactBody(t *testing.T) {
	server := sequenceServer(statusStep(500, `{"error":{"message":"context size exceeded"}}`))
	defer server.Close()
	endpoint := mustParse(t, server.URL)
	_, err := TranslateStream(ctxOf(t), endpoint, "hymt", DefaultGenParams(), "Translate", nil)
	if err == nil || !strings.Contains(err.Error(), "context size exceeded") {
		t.Fatalf("err = %v", err)
	}
}

func TestCancellingReturnsThePartialTranslation(t *testing.T) {
	server := sequenceServer(func(w http.ResponseWriter) {
		w.WriteHeader(200)
		flusher, _ := w.(http.Flusher)
		for i := 0; i < 40; i++ {
			fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"가\"}}]}\n\n")
			flusher.Flush()
			time.Sleep(20 * time.Millisecond)
		}
	})
	defer server.Close()
	endpoint := mustParse(t, server.URL)

	ctx, cancel := context.WithCancel(context.Background())
	first := true
	result, err := TranslateStream(ctx, endpoint, "hymt", DefaultGenParams(), "Translate", func(delta string) {
		if first {
			first = false
			cancel()
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Cancelled || result.Text == "" {
		t.Fatalf("cancelled = %v text = %q", result.Cancelled, result.Text)
	}
}
