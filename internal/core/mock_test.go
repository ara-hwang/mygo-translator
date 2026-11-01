package core

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func ctxOf(t *testing.T) context.Context {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	t.Cleanup(cancel)
	return ctx
}

// statusStep writes a single plain response for a health probe or an error
// path.
func statusStep(code int, body string) func(w http.ResponseWriter) {
	return func(w http.ResponseWriter) {
		w.WriteHeader(code)
		fmt.Fprint(w, body)
	}
}

// jsonStep writes a 200 JSON response.
func jsonStep(body string) func(w http.ResponseWriter) {
	return statusStep(http.StatusOK, body)
}

// sseStep writes a 200 chunked response of the given SSE events, one
// "data: <event>\n\n" per element.
func sseStep(events ...string) func(w http.ResponseWriter) {
	return func(w http.ResponseWriter) {
		w.WriteHeader(http.StatusOK)
		flusher, _ := w.(http.Flusher)
		for _, event := range events {
			fmt.Fprintf(w, "data: %s\n\n", event)
			if flusher != nil {
				flusher.Flush()
			}
		}
	}
}

// sequenceServer answers each request with the steps in order, repeating
// the last step once the list is exhausted.
func sequenceServer(steps ...func(w http.ResponseWriter)) *httptest.Server {
	index := 0
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if index >= len(steps) {
			index = len(steps) - 1
		}
		steps[index](w)
		index++
	}))
}

// mustParse parses a test server URL into an Endpoint.
func mustParse(t *testing.T, rawURL string) Endpoint {
	endpoint, err := ParseEndpoint(rawURL)
	if err != nil {
		t.Fatal(err)
	}
	return endpoint
}
