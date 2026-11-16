package app

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"offline-translator/internal/core"
)

// StartDownload begins downloading a catalog model with resume support.
func (a *App) StartDownload(option core.ModelOption) {
	a.Mu.Lock()
	if a.DownloadID != "" {
		a.Mu.Unlock()
		return
	}
	a.DownloadID = option.ID
	a.Download = core.DownloadProgress{Total: option.SizeBytes}
	base := a.settings.DownloadBase
	dest := filepath.Join(a.paths.ModelsDir(), option.FileName())
	a.Mu.Unlock()
	a.Invalidate()

	go a.runDownload(option, option.DownloadURL(base), dest, core.PartPath(dest))
}

// runDownload streams the file, resuming from the .part file when present.
func (a *App) runDownload(option core.ModelOption, url, dest, part string) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	a.cancelMu.Lock()
	a.downloadC = cancel
	a.cancelMu.Unlock()

	progress := int64(0)
	if info, err := os.Stat(part); err == nil {
		progress = info.Size()
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		a.finishDownload(fmt.Sprintf("다운로드를 시작하지 못했습니다: %v", err))
		return
	}
	if progress > 0 {
		req.Header.Set("Range", "bytes="+strconv.FormatInt(progress, 10)+"-")
	}
	resp, doErr := http.DefaultClient.Do(req)
	if doErr != nil {
		if ctx.Err() != nil {
			a.finishDownload("다운로드를 취소했습니다.")
		} else {
			a.finishDownload(fmt.Sprintf("다운로드를 시작하지 못했습니다: %v", doErr))
		}
		return
	}
	defer resp.Body.Close()
	if progress > 0 && resp.StatusCode != http.StatusPartialContent {
		progress = 0 // restart when the host cannot resume
	}
	total := progress + resp.ContentLength
	if resp.ContentLength <= 0 {
		total = option.SizeBytes
	}
	if total < progress {
		total = progress
	}

	flag := os.O_WRONLY | os.O_CREATE | os.O_TRUNC
	if progress > 0 {
		flag = os.O_WRONLY | os.O_CREATE | os.O_APPEND
	}
	out, err := os.OpenFile(part, flag, 0o644)
	if err != nil {
		a.finishDownload(fmt.Sprintf("파일을 열지 못했습니다: %v", err))
		return
	}

	buf := make([]byte, 256*1024)
	written := progress
	var lastInvalidate time.Time
	for {
		nn, readErr := resp.Body.Read(buf)
		if nn > 0 {
			if _, werr := out.Write(buf[:nn]); werr != nil {
				out.Close()
				a.finishDownload(fmt.Sprintf("파일에 쓰지 못했습니다: %v", werr))
				return
			}
			written += int64(nn)
			if time.Since(lastInvalidate) > 150*time.Millisecond {
				lastInvalidate = time.Now()
				a.Mu.Lock()
				a.Download = core.DownloadProgress{Downloaded: written, Total: total}
				a.Mu.Unlock()
				a.Invalidate()
			}
		}
		if ctx.Err() != nil {
			out.Close()
			os.Remove(part)
			a.Mu.Lock()
			a.Notice = "다운로드를 취소했습니다."
			a.Mu.Unlock()
			a.finishDownload("")
			return
		}
		if readErr != nil {
			out.Close()
			if !errors.Is(readErr, io.EOF) {
				a.finishDownload(fmt.Sprintf("다운로드 중 오류: %v", readErr))
				return
			}
			break
		}
	}
	if err := os.Rename(part, dest); err != nil {
		a.finishDownload(fmt.Sprintf("완성 파일을 옮기지 못했습니다: %v", err))
		return
	}
	a.refreshLocalModels()
	a.finishDownload("")
	mygo.NewNotification(mygo.NotificationOptions{
		Title: "다운로드 완료",
		Body:  option.FileName(),
	}).Show()
}

func (a *App) finishDownload(msg string) {
	a.Mu.Lock()
	a.DownloadID = ""
	a.Download = core.DownloadProgress{}
	if msg != "" {
		a.Notice = msg
		a.Tab = tabModels
	}
	a.Mu.Unlock()
	a.Invalidate()
}

// CancelDownload cancels the running download; the .part file is removed.
func (a *App) CancelDownload() {
	a.cancelMu.Lock()
	cancel := a.downloadC
	a.cancelMu.Unlock()
	if cancel != nil {
		cancel()
	}
}
