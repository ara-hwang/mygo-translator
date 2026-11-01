package core

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"
)

// Timeouts for one translation request and one health probe.
const (
	TranslateTimeout = 300 * time.Second
	HealthTimeout    = 3 * time.Second
)

// Endpoint is the location of a llama-server instance (loopback in practice).
type Endpoint struct {
	Host string
	Port int
	// Scheme is http or https.
	Scheme string
}

// NewEndpoint builds an endpoint from a host and port.
func NewEndpoint(host string, port int) Endpoint {
	return Endpoint{Host: host, Port: port, Scheme: "http"}
}

// LocalhostEndpoint is 127.0.0.1:<port>.
func LocalhostEndpoint(port int) Endpoint {
	return NewEndpoint("127.0.0.1", port)
}

// ParseEndpoint parses "host:port", "http://host:port" or "host" (defaults
// to port 8788).
func ParseEndpoint(text string) (Endpoint, error) {
	trimmed := strings.TrimSpace(text)
	if trimmed == "" {
		return Endpoint{}, fmt.Errorf("서버 주소가 비어 있습니다.")
	}
	scheme := "http"
	rest := trimmed
	if strings.HasPrefix(rest, "https://") {
		scheme = "https"
		rest = rest[len("https://"):]
	} else if strings.HasPrefix(rest, "http://") {
		rest = rest[len("http://"):]
	}
	hostPort := rest
	if i := strings.IndexByte(rest, '/'); i >= 0 {
		hostPort = rest[:i]
	}
	if i := strings.LastIndexByte(hostPort, ':'); i >= 0 {
		host, portText := hostPort[:i], hostPort[i+1:]
		port, err := strconv.Atoi(portText)
		if err != nil || port < 1 || port > 65535 {
			return Endpoint{}, fmt.Errorf("포트 번호가 올바르지 않습니다: %q", portText)
		}
		if host == "" {
			return Endpoint{}, fmt.Errorf("호스트가 비어 있습니다.")
		}
		return Endpoint{Host: host, Port: port, Scheme: scheme}, nil
	}
	if hostPort == "" {
		return Endpoint{}, fmt.Errorf("호스트가 비어 있습니다.")
	}
	return Endpoint{Host: hostPort, Port: 8788, Scheme: scheme}, nil
}

// Resolved maps "localhost" onto the loopback address.
func (e Endpoint) Resolved() Endpoint {
	host := e.Host
	if host == "localhost" {
		host = "127.0.0.1"
	}
	return Endpoint{Host: host, Port: e.Port, Scheme: e.Scheme}
}

// BaseURL is the human-readable base URL.
func (e Endpoint) BaseURL() string {
	return fmt.Sprintf("%s://%s:%d", e.Scheme, e.Host, e.Port)
}

// String renders "host:port".
func (e Endpoint) String() string {
	return fmt.Sprintf("%s:%d", e.Host, e.Port)
}

// GenerationParams carries the sampling options sent with each request.
type GenerationParams struct {
	Temperature   float64
	TopP          float64
	TopK          int
	RepeatPenalty float64
	MaxTokens     int
}

// GenParamsFrom converts generation settings into request parameters.
func GenParamsFrom(settings GenerationSettings) GenerationParams {
	return GenerationParams{
		Temperature:   settings.Temperature,
		TopP:          settings.TopP,
		TopK:          settings.TopK,
		RepeatPenalty: settings.RepeatPenalty,
		MaxTokens:     settings.MaxTokens,
	}
}

// DefaultGenParams matches the model-card defaults.
func DefaultGenParams() GenerationParams {
	return GenParamsFrom(DefaultGenerationSettings())
}

// Health is the result of a health probe.
type Health struct {
	Ready  bool
	Detail string
}

// ChatError is one of the failures a translation request can surface.
type ChatError struct {
	Kind    ChatErrorKind
	Message string
}

// ChatErrorKind discriminates transport, server and protocol failures.
type ChatErrorKind int

const (
	ChatErrorHTTP ChatErrorKind = iota
	ChatErrorServer
	ChatErrorProtocol
)

func (e *ChatError) Error() string {
	switch e.Kind {
	case ChatErrorServer:
		return "모델 서버 오류: " + e.Message
	case ChatErrorProtocol:
		return "스트림 해석 오류: " + e.Message
	default:
		return e.Message
	}
}

func httpChatError(err error) *ChatError {
	return &ChatError{Kind: ChatErrorHTTP, Message: err.Error()}
}

// TranslationResult is everything learned from one translation request.
type TranslationResult struct {
	// Text is the cleaned translation (possibly partial when cancelled).
	Text string
	// Cancelled is true when the caller cancelled the request.
	Cancelled bool
	// FinishReason is why generation stopped, as reported by the server.
	FinishReason string
	// PromptTokens / CompletionTokens when the server reports usage.
	PromptTokens     int64
	CompletionTokens int64
	// FirstTokenMs is the milliseconds until the first token arrived.
	FirstTokenMs int64
	// TotalMs is the total wall-clock milliseconds.
	TotalMs int64
	// TokensPerSecond is the server-reported speed, if present.
	TokensPerSecond float64

	raw string // streamed deltas before normalization
}

// ProbeHealth asks the server whether it is ready to translate. A 503 while
// the model loads is a normal answer, not an error, so this only fails when
// the connection itself fails.
func ProbeHealth(ctx context.Context, endpoint Endpoint, timeout time.Duration) (Health, error) {
	client := &http.Client{}
	url := endpoint.Resolved().BaseURL() + "/health"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return Health{}, httpChatError(err)
	}
	if timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, timeout)
		defer cancel()
		req = req.WithContext(ctx)
	}
	resp, err := client.Do(req)
	if err != nil {
		return Health{}, httpChatError(err)
	}
	defer resp.Body.Close()
	body := limitedRead(resp.Body, 8192)
	ready := resp.StatusCode == http.StatusOK && strings.Contains(body, "\"ok\"")
	var detail string
	switch {
	case ready:
		detail = "ok"
	case resp.StatusCode == http.StatusServiceUnavailable:
		detail = strings.TrimSpace(body)
	case strings.TrimSpace(body) == "":
		detail = fmt.Sprintf("HTTP %d", resp.StatusCode)
	default:
		detail = strings.TrimSpace(body)
	}
	return Health{Ready: ready, Detail: detail}, nil
}

// TranslateStream requests a streaming translation and calls onDelta for
// every content delta as it arrives, letting the UI paint text while the
// model is still generating. Cancelling the context returns the partial
// text with Cancelled set.
func TranslateStream(
	ctx context.Context,
	endpoint Endpoint,
	model string,
	params GenerationParams,
	prompt string,
	onDelta func(string),
) (*TranslationResult, error) {
	started := time.Now()
	result := &TranslationResult{}

	payload, err := json.Marshal(map[string]any{
		"model":          model,
		"messages":       []map[string]string{{"role": "user", "content": prompt}},
		"temperature":    params.Temperature,
		"top_p":          params.TopP,
		"top_k":          params.TopK,
		"repeat_penalty": params.RepeatPenalty,
		"max_tokens":     params.MaxTokens,
		"stream":         true,
		"stream_options": map[string]bool{"include_usage": true},
		"cache_prompt":   true,
	})
	if err != nil {
		return nil, httpChatError(err)
	}

	client := &http.Client{}
	url := endpoint.Resolved().BaseURL() + "/v1/chat/completions"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(payload))
	if err != nil {
		return nil, httpChatError(err)
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := client.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			result.Cancelled = true
			result.Text = NormalizeOutput(result.raw)
			result.TotalMs = time.Since(started).Milliseconds()
			return result, nil
		}
		return nil, httpChatError(err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		detail := limitedRead(resp.Body, 8192)
		return nil, &ChatError{Kind: ChatErrorHTTP,
			Message: fmt.Sprintf("HTTP %d: %s", resp.StatusCode, compactErrorBody(detail))}
	}

	var dataLines []string
	firstDeltaSeen := false
	scanner := bufio.NewScanner(resp.Body)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	dispatch := func(payload string) error {
		if payload == "[DONE]" {
			return io.EOF
		}
		var value map[string]any
		if err := json.Unmarshal([]byte(payload), &value); err != nil {
			return &ChatError{Kind: ChatErrorProtocol,
				Message: fmt.Sprintf("%v: %s", err, truncateRunes(payload, 200))}
		}
		if message := serverErrorMessage(value); message != "" {
			return &ChatError{Kind: ChatErrorServer, Message: message}
		}
		if choices, ok := value["choices"].([]any); ok && len(choices) > 0 {
			choice, _ := choices[0].(map[string]any)
			if choice != nil {
				if delta, ok := choice["delta"].(map[string]any); ok {
					if content, ok := delta["content"].(string); ok && content != "" {
						if !firstDeltaSeen {
							firstDeltaSeen = true
							ms := time.Since(started).Milliseconds()
							if ms < 1 {
								ms = 1
							}
							result.FirstTokenMs = ms
						}
						result.raw += content
						if onDelta != nil {
							onDelta(content)
						}
					}
				}
				if reason, ok := choice["finish_reason"].(string); ok && reason != "" {
					result.FinishReason = reason
				}
			}
		}
		if usage, ok := value["usage"].(map[string]any); ok {
			result.PromptTokens = anyToInt64(usage["prompt_tokens"])
			result.CompletionTokens = anyToInt64(usage["completion_tokens"])
		}
		if timings, ok := value["timings"].(map[string]any); ok {
			result.TokensPerSecond = anyToFloat64(timings["predicted_per_second"])
		}
		return nil
	}

	done := false
	// An SSE event is one or more "data:" lines separated from the next
	// event by a blank line.
	flush := func() (stop bool, err error) {
		if len(dataLines) == 0 {
			return false, nil
		}
		payload := strings.Join(dataLines, "\n")
		dataLines = nil
		if payload == "" {
			return false, nil
		}
		if payload == "[DONE]" {
			return true, nil
		}
		err = dispatch(payload)
		return false, err
	}

scan:
	for scanner.Scan() {
		line := scanner.Text()
		switch {
		case line == "":
			stop, err := flush()
			if err != nil {
				return nil, err
			}
			if stop {
				done = true
				break scan
			}
		case strings.HasPrefix(line, "data:"):
			dataLines = append(dataLines, strings.TrimSpace(strings.TrimPrefix(line, "data:")))
		default:
			// comments, event:, id: lines — ignored
		}
	}
	if !done {
		if _, err := flush(); err != nil {
			return nil, err
		}
	}
	result.Cancelled = ctx.Err() != nil
	result.Text = NormalizeOutput(result.raw)
	result.TotalMs = time.Since(started).Milliseconds()
	return result, nil
}

func serverErrorMessage(value map[string]any) string {
	errorField, ok := value["error"].(map[string]any)
	if !ok {
		return ""
	}
	if message, ok := errorField["message"].(string); ok && message != "" {
		return message
	}
	raw, _ := json.Marshal(errorField)
	return string(raw)
}

func compactErrorBody(body string) string {
	trimmed := strings.TrimSpace(body)
	var value map[string]any
	if err := json.Unmarshal([]byte(trimmed), &value); err == nil {
		if message := serverErrorMessage(value); message != "" {
			return message
		}
	}
	return truncateRunes(trimmed, 300)
}

func truncateRunes(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return string(runes[:max]) + "…"
}

func anyToInt64(v any) int64 {
	if n, ok := v.(float64); ok {
		return int64(n)
	}
	return 0
}

func anyToFloat64(v any) float64 {
	if n, ok := v.(float64); ok {
		return n
	}
	return 0
}

// limitedRead reads at most max bytes.
func limitedRead(r io.Reader, max int) string {
	raw, _ := io.ReadAll(io.LimitReader(r, int64(max)))
	return string(raw)
}
