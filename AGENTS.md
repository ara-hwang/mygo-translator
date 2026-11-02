# AGENTS.md

Windows 11용 오프라인 번역기 HyMT 저장소. 제품 소개·사용법은 [README.md](README.md) 참고.

## 명령어

```powershell
cargo build                      # 개발 빌드
cargo run --release              # 앱 실행 (GPUI DirectX 셰이더 컴파일로 fxc.exe 필요, 아래 참고)
cargo test -p hymt-core          # 코어 테스트 전부 여기 (현재 65개)
cargo test -p hymt-app           # 앱 단위 테스트 (트레이 아이콘, 자동 실행 경로 등, 현재 7개)
cargo fmt --all                  # 포맷
cargo clippy --workspace --all-targets
```

- llama.cpp 런타임 내려받기: `powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1 -Variant cpu|cuda|vulkan`
- 릴리스 빌드는 Windows SDK의 `fxc.exe`를 쓴다. 못 찾으면 `GPUI_FXC_PATH` 환경 변수로 지정. 디버그 빌드는 불필요.
- 외부 런타임·모델 없이 `cargo test -p hymt-core`는 어디서든 돈다.
- PATH에 llama-server가 있으면 코어의 서버 탐색 테스트(`engine::discover_server`)가 PATH를 탐색해 실패한다. PATH를 정리한 뒤 돌린다.

## 아키텍처

Cargo workspace, 크레이트 2개:

- `crates/hymt-core` — 플랫폼 독립 코어. **의존성은 `serde`, `serde_json` + std만 허용.** UI·네트워크 크레이트·OS 종속 코드를 넣지 않는다. 프롬프트 구성, 스트림 파싱, 프로세스 감독, 설정 검증 등 판단이 필요한 로직은 전부 여기에 두고 테스트한다.
  - `engine.rs` — llama-server 탐색·실행 인자·`/health` 대기·프로세스 감독
  - `prompt.rs` — Tencent Hy-MT2 모델 카드의 **공식 프롬프트 템플릿 4종**(기본/용어집/스타일/구분자). 임의로 바꾸지 않는다. 중국어 대상 언어는 중국어 템플릿 자동 선택.
  - `http.rs`, `sse.rs` — 의존성 없는 HTTP/1.1 클라이언트와 증분 SSE 디코더(멀티바이트 경계, 취소, 타임아웃)
  - `settings.rs` — 설정 검증·원자적 저장. `paths.rs` — 설치/포터블(`portable.txt`) 경로 해석. 하드코딩 경로 금지, 둘을 경유.
  - `models.rs` — Tencent 공식 GGUF 카탈로그·다운로드
- `crates/hymt-app` — GPUI UI 바이너리(`hymt`). `gpui-kit` 컴포넌트 사용. 화면은 표시와 이벤트만 담당하고 판단 로직은 코어에 둔다.
  - `root.rs` — 앱 상태·엔진 감독·스트리밍·다운로드·트레이/자동 실행 처리. `ui_*.rs`, `widgets.rs`, `smooth_scroll.rs` — 화면 표시와 이벤트.
  - `tray.rs` — 알림 영역 아이콘과 메뉴. `window_ops.rs` — Win32 창 숨김·복원. `autostart.rs` — HKCU Run 키 등록·해제. Windows 종속 코드는 이 크레이트에만 둔다.
  - `--autostart` 인자로 시작하면 트레이 옵션에 따라 창을 숨기고 시작한다.

## 테스트

- 각 코어 소스 파일에 `#[cfg(test)] mod tests`를 인라인으로 배치하는 것이 관행. 새 모듈도 동일하게.
- 로직은 hymt-core에 두고 테스트로 고정한다. 특히 `prompt.rs` 수정은 모델 카드 원문과 일치하는지 테스트로 검증.
- 코어에 넣을 수 없는 OS 종속 로직(트레이, 창 숨김, 레지스트리)은 hymt-app에 두고 같은 인라인 테스트 방식으로 검증한다.
- GPUI UI(`hymt-app`)는 Windows에서만 실행 검증 가능. UI 변경 시 `cargo build` 성공만으로 끝내지 말고 동작 확인 여부를 명시.

## 플랫폼

- 대상: Windows 11 x64, rust-version 1.90 (workspace 고정).
- hymt-core는 비-Windows에서도 컴파일·테스트된다. OS 분기가 필요하면 hymt-core에 두되 최소화.
- hymt-app은 Windows 전용이다. Win32 접근은 `windows`, `winreg`, `tray-icon` 크레이트로만 하고 직접 FFI는 쓰지 않는다.
- 릴리스 빌드는 GUI 서브시스템이라 콘솔 출력이 보이지 않는다. 진단은 앱 로그(`logs/hymt.log`)와 llama-server 로그로 확인.

## 규칙

- 커밋 메시지: Conventional Commits + 한국어 개조식. 예: `fix: 빈 에디터 클릭 패닉 수정`, `feat: UI를 gpui-kit 컴포넌트로 전환`. 서명 트레일러 없음.
- 문서(README 등)는 한국어. 코드 주석·doc comment는 영어.
- 생성 파라미터 기본값(temperature 0.7, top_p 0.6, top_k 20, repetition_penalty 1.05, max_tokens 4096)은 Tencent 권장값이다. 임의 변경 금지.
- 모델 카탈로그는 Tencent 공식 저장소 파일만 추가하고, 파일명·크기는 Hugging Face API로 확인한 값을 쓴다.
