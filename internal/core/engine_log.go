package core

import (
	"strings"
	"sync"
)

// engineLogTail retains recent output shared by the stdout and stderr pumps.
// Its zero value is ready for use.
type engineLogTail struct {
	mu sync.Mutex
	s  []string
}

func (tail *engineLogTail) reset() {
	tail.mu.Lock()
	defer tail.mu.Unlock()
	tail.s = tail.s[:0]
}

func (tail *engineLogTail) append(line string) {
	tail.mu.Lock()
	defer tail.mu.Unlock()
	tail.s = append(tail.s, line)
	if len(tail.s) > TailLines {
		tail.s = tail.s[len(tail.s)-TailLines:]
	}
}

func (tail *engineLogTail) summary() string {
	tail.mu.Lock()
	defer tail.mu.Unlock()
	if len(tail.s) == 0 {
		return ""
	}
	joined := strings.Join(tail.s[max(0, len(tail.s)-3):], " / ")
	runes := []rune(joined)
	if len(runes) > 400 {
		joined = string(runes[:400])
	}
	return joined
}
