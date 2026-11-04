# AGENTS.md

Windows 11용 오프라인 번역기 Offline Translator 저장소. 제품 소개·사용법은 [README.md](README.md) 참고.

## 명령어

```powershell
go build .                            # 개발 빌드 (./offline-translator.exe)
go test ./...                         # 코어 테스트 전부 internal/core (PATH에 llama-server가 있으면 서버 탐색 테스트만 실패함)
gofmt -l .                            # 포맷 확인 (-w로 수정)
go vet ./...
go build -trimpath -ldflags "-s -w -H windowsgui" -o dist\offline-translator.exe .   # 릴리스 빌드
```

- llama.cpp 런타임 내려받기: `powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1 -Variant cpu|cuda|vulkan`
- Go 1.27+이면 별도의 빌드 도구 설치가 필요 없다.
- exe 아이콘은 저장소 커밋된 `rsrc_windows_amd64.syso`가 `go build` 시 자동으로 링크된다. `resources/icon.ico`를 바꾸면 다음 명령으로 `.syso`를 다시 만들어 커밋한다:
  ```powershell
  go tool rsrc -arch amd64 -ico resources/icon.ico -o rsrc_windows_amd64.syso
  ```
- 윈도우에서 UI를 검증할 때는 release 빌드를 실행해 화면을 직접 확인한다.
- PATH에 llama-server가 있으면 코어의 서버 탐색 테스트(`DiscoverServer`)가 PATH를 탐색해 실패한다. PATH를 정리한 뒤 돌린다.

## 아키텍처

바이너리는 `offline-translator.exe`(Windows GUI 서브시스템). UI는 [mygo](https://github.com/egoist/mygo) 네이티브 위젯을 쓴다.

- `internal/core` — 플랫폼 독립 코어. **의존성은 Go 표준 라이브러리 + `golang.org/x/sys`만 허용.** UI·mygo 종속 코드를 넣지 않는다. 프롬프트 구성, 스트림 파싱, 프로세스 감독, 설정 검증 등 판단이 필요한 로직은 전부 여기에 두고 테스트한다.
  - `prompt.go` — Tencent Hy-MT2 모델 카드의 **공식 프롬프트 템플릿 4종**(기본/용어집/스타일/구분자). 임의로 바꾸지 않는다. 중국어 대상 언어는 중국어 템플릿 자동 선택.
  - `server.go` — OpenAI 호환 스트리밍 번역 클라이언트, `/health` 대기
  - `engine.go` — llama-server 탐색·실행 인자·프로세스 감독. `engine_windows.go` / `engine_other.go` — 빌드 태그 분기
  - `settings.go` — 설정 검증·원자적 저장. `paths.go` — 설치/포터블 경로 해석. 하드코딩 경로 금지, 둘을 경유.
  - `models.go` — Tencent 공식 GGUF 카탈로그·다운로드
  - `lang.go`, `glossary.go`, `normalize.go` — 언어표·용어집 파서·출력 정규화
- `internal/app` — mygo UI(`app.Run`). 화면은 표시와 이벤트만 담당하고 판단 로직은 코어에 둔다.
  - `app.go` — 앱 상태·엔진 감독·스트리밍·다운로드·전역 단축키·트레이/자동 실행 연결
  - `ui.go` — 번역/모델/설정 탭 화면
  - `assets.go` — `assets/` 아이콘 PNG embed(`internal/app/assets`)
- `internal/winapi` — mygo가 감싸지 않는 Win32 조각(선택 텍스트 복사 합성, 클립보드). 빌드 태그 `windows`만.

## 데이터 공유

`%LOCALAPPDATA%\offline-translator\`에 `runtime\`, `models\`, `settings.json`을 저장한다. 기존 `%LOCALAPPDATA%\HyMT\` 데이터는 자동 이동하거나 복사하지 않는다. `settings.json` 필드 이름은 그대로 유지(`server`, `hotkey.combo`의 `ctrl/shift/alt/win/key` 등).

## 테스트

- 코어 로직은 `internal/core`의 `_test.go` 인라인 테스트로 고정한다. 특히 `prompt.go` 수정은 모델 카드 원문과 일치하는지 테스트로 검증.
- OS 종속 로직(`internal/winapi`, 트레이)은 자동화 범위 밖. Windows에서 설정 탭 단축키 스위치 등으로 직접 확인하고, 동작 확인 여부를 결과에 명시한다.
- UI(`internal/app`)는 Windows에서만 실행 검증 가능. `go build` 성공만으로 끝내지 말고 동작 확인 여부를 명시.

## 플랫폼

- 대상: Windows 11 x64, go.mod의 Go 1.27 이상.
- OS 분기는 빌드 태그 파일(`engine_windows.go` / `engine_other.go`, `capture_windows.go` / `capture_other.go`)로 분리한다.
- Win32 접근은 `internal/winapi`에서 syscall로만 하고, 나머지는 mygo 공개 API로만 쓴다.
- 릴리스 빌드는 GUI 서브시스템이라 콘솔 출력이 보이지 않는다. 진단은 llama-server 로그(엔진 로그는 모델 탭)와 `%LOCALAPPDATA%\offline-translator\logs\`에서 확인.

## 규칙

- 커밋 메시지: Conventional Commits + 한국어 개조식. 예: `fix: 빈 원문 확인 제거`. 서명 트레일러 없음.
- 문서(README 등)는 한국어. 코드 주석·doc comment는 영어.
- 생성 파라미터 기본값(temperature 0.7, top_p 0.6, top_k 20, repetition_penalty 1.05, max_tokens 4096)은 Tencent 권장값이다. 임의 변경 금지.
- 모델 카탈로그는 Tencent 공식 저장소 파일만 추가하고, 파일명·크기는 Hugging Face API로 확인한 값을 쓴다.
