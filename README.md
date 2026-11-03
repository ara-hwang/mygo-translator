# HyMT — Windows 11용 오프라인 번역기 (GPUI + Tencent Hy-MT2)

GPUI(Rust UI 프레임워크)로 만든 데스크톱 번역기입니다. 추론은 Tencent의 최신 번역 모델 계열인 **Hy-MT2**(1.8B / 7B / 30B-A3B)를 llama.cpp로 로컬에서 실행하며, 인터넷 연결 없이 동작합니다. 네트워크는 모델 파일을 한 번 내려받을 때만 사용합니다.

```
┌─ HyMT ─────────────────────────────────────────────── 엔진: 준비됨 ─┐
│  [한국어 ▾]  ⇄  [영어 ▾]              [용어집 3개] [스타일 · 격식체] │
│  ┌ 원문 ─────────────┐  ┌ 번역 ─────────────────────────┐         │
│  │ GPUI is fast.     │  │ GPUI는 빠릅니다.               │         │
│  └───────────────────┘  └───────────────────────────────┘         │
│                                        Ctrl+Enter로 번역 [번역]      │
└───────────────────────────────────────────────────────────────────┘
```

## 주요 기능

- **완전 오프라인 추론**: 모델 서버는 `127.0.0.1`에만 바인딩되며 외부로 나가는 요청이 없습니다.
- **스트리밍 번역**: 토큰이 생성되는 즉시 화면에 표시되고, 중단할 수 있습니다.
- **단축키 선택 번역**: 다른 앱에서 텍스트를 선택한 상태로 단축키를 누르면 HyMT 창이 열리면서 바로 번역합니다. 조합 단축키(기본 `Ctrl+Shift+C`)와 같은 키를 빠르게 두 번 누르는 방식(기본 `Ctrl+C C`, DeepL 방식)을 모두 지원합니다. 언어 쌍은 주 창에 선택된 값을 그대로 사용합니다. 설정 → 단축키에서 켤 수 있습니다.
- **Hy-MT2 공식 프롬프트 4종**: 기본 번역, **용어집(terminology intervention)**, **스타일 지정**, **구분자 보존**. 지시문 언어(자동/영어/중국어)도 선택할 수 있습니다.
- **38개 언어**: 모델 카드에 명시된 33개 언어 + 번체 중국어, 광둥어, 티베트어, 카자흐어, 몽골어, 위구르어.
- **모델 관리 내장**: Tencent 공식 GGUF 저장소에서 검증된 파일 크기와 함께 원클릭 다운로드(이어받기 지원), 로컬 `.gguf` 자동 스캔.
- **엔진 감독**: llama.cpp 서버 프로세스를 앱이 직접 관리하고, `/health`가 준비될 때까지 상태를 보여 줍니다. 크래시해도 UI는 살아 있습니다.
- **IME 지원 에디터**: GPUI 0.2에 텍스트 입력 위젯이 없어, 한글 IME 조합·선택·소프트 랩·스크롤을 지원하는 에디터를 직접 구현했습니다(`crates/hymt-app/src/text_area.rs`).
- **트레이·자동 실행**: 설정에서 창을 닫아도 트레이 아이콘으로 유지하거나 Windows 시작 시 자동 실행할 수 있습니다. 트레이 아이콘을 왼쪽 클릭하면 창이 다시 열립니다.
- **이식성**: `portable.txt`를 exe 옆에 두면 모든 데이터가 exe 폴더에 저장됩니다.

## 요구 사항 (Windows 11)

| 항목 | 요구 사항 |
| --- | --- |
| OS | Windows 11 x64 |
| Rust | 소스 빌드 시 1.93 이상 (stable) |
| Go | mygo 포트 빌드 시 1.27 이상 (`go.mod` 기준) |
| 빌드 도구 | Visual Studio Build Tools의 "Desktop development with C++" + Windows 11 SDK |
| 런타임 | `llama-server.exe` (llama.cpp Windows 빌드) — 앱에 포함되지 않습니다 |
| 디스크 | 모델 1.06GB(1.8B Q4) ~ 8GB(7B Q8) + 런타임 약 100MB |
| 메모리 | 1.8B Q4 기준 3GB 이상, 7B Q4 기준 8GB 이상 권장 |

> 릴리스 빌드는 GPUI가 DirectX 셰이더를 컴파일하므로 Windows SDK의 `fxc.exe`가 필요합니다. `fxc.exe` 위치를 자동으로 찾지 못하면 `GPUI_FXC_PATH` 환경 변수로 지정하세요. 디버그 빌드(`cargo run`)는 `fxc.exe` 없이 동작합니다.

## 빠른 시작

1. **앱 실행**
   GitHub Releases에서 설치 마법사(`-setup.exe`)를 실행하거나 휴대용 ZIP을 압축 해제한 뒤 `hymt.exe`를 실행합니다. 또는 소스에서 빌드합니다.
   ```powershell
   git clone <this-repo> hymt
   cd hymt
   cargo run --release
   ```
   첫 빌드는 GPUI와 의존성 때문에 몇 분 걸립니다.

2. **llama.cpp 런타임 준비**  
   [llama.cpp Releases](https://github.com/ggml-org/llama.cpp/releases)에서 Windows용 zip을 받아 압축을 풀고, `llama-server.exe`와 같은 폴더의 DLL들을 `%LOCALAPPDATA%\HyMT\runtime\`에 넣습니다.  
   자동으로 받으려면:
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1 -Variant cpu
   ```
   NVIDIA GPU라면 `-Variant cuda`, Vulkan은 `-Variant vulkan`을 사용합니다.

3. **모델 다운로드**  
   앱 실행 → **모델** 탭 → `Hy-MT2-1.8B-Q4_K_M.gguf`(권장) 옆의 **내려받기**. 다운로드가 끝나면 자동으로 엔진이 다시 시작됩니다.  
   사내망 등으로 다운로드가 안 되면 **설정 → 다운로드**에서 `hf-mirror.com`으로 바꾸거나, `.gguf` 파일을 직접 `%LOCALAPPDATA%\HyMT\models\`에 넣고 **새로고침**을 누르세요.

4. **번역**  
   원문을 입력하고 `Ctrl+Enter` 또는 **번역** 버튼.  
   다른 앱의 선택 텍스트를 번역하려면 **설정 → 단축키**에서 **전역 단축키 사용**을 켜세요. 기본 단축키는 두 가지입니다.
   - 조합 단축키 `Ctrl+Shift+C`: 다른 앱에서 텍스트를 선택한 상태로 누르면 HyMT 창이 열리고 바로 번역합니다.
   - 두 번 누르기 `Ctrl+C C`: 같은 키를 빠르게 두 번 누르는 방식(DeepL과 동일). 앱의 일반 `Ctrl+C` 복사에는 영향이 없습니다.
   기록 버튼으로 조합을 바꿀 수 있습니다. 두 트리거 모두 주 창에 선택된 언어 쌍으로 번역합니다.

## 모델 선택 가이드

Tencent 공식 GGUF 저장소의 파일만 카탈로그에 넣었습니다(파일명·크기는 Hugging Face API에서 확인).

| 모델 | 파일 | 크기 | 권장 상황 |
| --- | --- | --- | --- |
| Hy-MT2-1.8B | `Hy-MT2-1.8B-Q4_K_M.gguf` | 1.06 GB | **기본 권장**, CPU에서도 빠름 |
| Hy-MT2-1.8B | `Hy-MT2-1.8B-Q6_K.gguf` | 1.37 GB | 품질 약간 우선 |
| Hy-MT2-1.8B | `Hy-MT2-1.8B-Q8_0.gguf` | 1.78 GB | 품질 우선, RAM 4GB+ |
| Hy-MT2-7B | `Hy-MT2-7B-Q4_K_M.gguf` | 4.31 GB | 품질 우선, RAM 8GB+ 또는 GPU |
| Hy-MT2-7B | `HY-MT2-7B-Q6_K.gguf` | 5.74 GB | RAM 10GB+ |
| Hy-MT2-7B | `HY-MT2-7B-Q8_0.gguf` | 7.43 GB | RAM 12GB+ |

- **30B-A3B (MoE)** 도 지원됩니다. 앱이 모델 폴더를 스캔하므로 `tencent/Hy-MT2-30B-A3B-GGUF`에서 직접 받아 `models` 폴더에 넣으면 목록에 나타납니다. 4비트 기준 약 18GB의 메모리가 필요합니다.
- **1.25bit(약 440MB) 초경량 모델**은 llama.cpp의 STQ 커널([PR #22836](https://github.com/ggml-org/llama.cpp/pull/22836))이 포함된 빌드가 필요합니다. 일반 빌드에서는 로드되지 않을 수 있습니다.
- Hy-MT2 GGUF는 `--jinja` 채팅 템플릿을 사용합니다. 앱이 자동으로 전달하므로 직접 실행할 때도 `--jinja`를 넣으세요.

## 번역 지시문 (모델 카드 원문 기준)

프롬프트는 `tencent/Hy-MT2-1.8B-GGUF` 모델 카드의 공식 예시를 그대로 사용합니다. 핵심 규칙: **언어 이름은 약어가 아니라 전체 이름**을 쓰고, **중국어 지시문에는 중국어 이름, 영어 지시문에는 영어 이름**을 씁니다.

| 모드 | 영어 지시문(요약) |
| --- | --- |
| 기본 | `Translate the following text into {target}. Note that you should only output the translated result without any additional explanation:` |
| 용어집 | `Reference the following translations:` + `{src} translates to {dst}` 목록 + 기본 지시문 |
| 스타일 | `... the translation style must strictly conform to [{style}]:` |
| 구분자 | `... retain the exact same number of delimiters ...` |

중국어 대상(중국어·번체·광둥어)일 때는 자동으로 중국어 템플릿(`将以下文本翻译为{target}，注意只需要输出翻译后的结果，不要额外解释：`)을 사용합니다. 설정에서 고정할 수도 있습니다.

**용어집 형식** (설정 탭):
```text
GPUI => 지피유아이
Zed	제드
# 주석은 무시됩니다
```
최대 200개, 용어당 120자까지 반영됩니다.

## 저장 위치

- 기본: `%LOCALAPPDATA%\HyMT\` (`settings.json`, `models\`, `runtime\`, `logs\`)
- 이식 모드: exe 옆에 `portable.txt` 파일이 있거나 `models` 폴더가 있으면 exe 폴더를 사용합니다.

`settings.json`에는 언어 쌍, 생성 파라미터(기본값은 Tencent 권장값: temperature 0.7, top_p 0.6, top_k 20, repetition_penalty 1.05, max_tokens 4096), 프롬프트 옵션, 서버 옵션(ctx, threads, GPU layers, port), 트레이·자동 실행 옵션, 단축키 옵션(조합·두 번 누르기, 선택 텍스트 획득 방식)이 저장됩니다.

## 시작 및 트레이

설정 → **시작 및 트레이**에서 다음을 선택할 수 있습니다.

- **트레이 아이콘으로 유지**: 창을 닫아도 프로세스가 종료되지 않고 알림 영역에 남습니다. 아이콘 왼쪽 클릭은 창 열기, 오른쪽 클릭 메뉴는 열기·종료입니다.
- **Windows 시작 시 자동 실행**: 현재 실행 파일을 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`에 등록합니다. 관리자 권한이 필요 없으며, 앱을 옮긴 뒤 다시 실행하면 등록 경로가 갱신됩니다.
- 두 옵션이 모두 켜져 있으면 로그인 시 창 없이 트레이에서 시작합니다. 트레이 아이콘을 만들지 못하면 창을 자동으로 표시합니다.

## 문제 해결

| 증상 | 해결 |
| --- | --- |
| "llama-server를 찾을 수 없습니다" | `%LOCALAPPDATA%\HyMT\runtime\llama-server.exe` 존재 여부 확인, 또는 설정에서 런타임 경로 지정 |
| "모델을 300초 안에 불러오지 못했습니다" | 모델 탭의 서버 로그 확인. RAM 부족이면 더 작은 양자화(1.8B Q4) 사용 |
| 포트 충돌 | `settings.json`의 `server.port`를 다른 값으로 변경 후 엔진 재시작 |
| GPU를 쓰고 싶다 | 설정 → GPU layers를 `99`(전부) 또는 `-1`(자동)로. CUDA/Vulkan 빌드의 llama-server 필요 |
| 번역이 느림 | threads를 물리 코어 수에 맞추고, 1.8B Q4 모델 사용 |
| 단축키를 등록할 수 없음 | 다른 프로그램이 이미 같은 조합을 등록한 것입니다. 설정 · 단축키 화면의 상태 문구를 보고 다른 키 조합으로 기록하세요 |
| 단축키가 아무 동작도 안 함 | 찾는 앱이 관리자 권한으로 실행 중이면 보안(UIPI) 때문에 UI Automation과 Ctrl+C 모두 실패할 수 있습니다. 앱을 일반 권한으로 실행하거나 텍스트를 직접 복사해 사용하세요 |
| 일부 앱에서 선택을 읽지 못함 | UI Automation을 지원하지 않는 앱은 Ctrl+C 방식으로 읽습니다(기본). 설정에서 획득 방식을 바꿔 시도해 보세요 |
| 창이 안 뜸 | `cargo run`으로 실행해 콘솔 오류 확인. 릴리스 빌드라면 `fxc.exe`(Windows SDK) 필요 |

## 구조

```
crates/
  hymt-core/        플랫폼 독립 코어 (serde + std만 사용)
    lang.rs         38개 언어표, 스크립트 감지
    prompt.rs       Hy-MT2 공식 프롬프트 템플릿
    engine.rs       llama-server 탐색·실행 인자·/health 대기·프로세스 감독
    http.rs         의존성 없는 HTTP/1.1 클라이언트 (chunked, 취소, 타임아웃)
    sse.rs          증분 SSE 디코더 (멀티바이트 경계 처리)
    client.rs       OpenAI 호환 스트리밍 번역 클라이언트
    models.rs       공식 GGUF 카탈로그·curl 다운로드·진행률
    settings.rs     설정 검증·원자적 저장
    paths.rs        설치/포터블 경로 해석
    glossary.rs     용어집 파서
  hymt-app/         GPUI UI
    text_area.rs    멀티라인 에디터 (IME, 선택, 소프트 랩, 스크롤)
    wrap.rs         순수 소프트 랩 알고리즘 (테스트됨)
    root.rs         상태·엔진 감독·스트리밍·다운로드
    hotkey.rs       전역 단축키 (RegisterHotKey + 저수준 키 입력 훅)
    capture.rs      선택 텍스트 캡처 (UIA 우선, Ctrl+C 클립보드 폴백)
    ui_*.rs         번역/모델/설정/언어 선택 화면
```

### 검증 상태

- `cargo test -p hymt-core`: **73개 테스트 통과** (프롬프트, 언어표, HTTP/SSE 청크 분할, 취소, 타임아웃, 헬스 대기, 모델 선택, 설정 검증(단축키 포함), 경로, 용어집, 출력 정규화). PATH에 llama-server가 있으면 서버 탐색 테스트만 실패합니다(상단 참고).
- `cargo test -p hymt-app`: **24개 테스트 통과** (트레이 아이콘, 자동 실행 경로, 단축키 조합 파서·두 번 누르기 상태머신, 클립보드 캡처 검증).
- 단축키의 실제 등록·트리거·팝업 동작은 Windows 전용이라 이 저장소의 자동화 범위 밖입니다. Windows에서 설정 → 단축키 화면으로 직접 확인하세요.

## mygo(Go) 포트

같은 동작을 Go 네이티브 UI 라이브러리인 **mygo**로 포팅한 바이너리도 이 저장소에서 함께 관리합니다. 코어 로직(프롬프트, 설정 검증, 스트리밍 클라이언트, 엔진 감독, 다운로드)과 사용자 경험은 Rust 버전과 동일하고, 구조는 `crates/hymt-app`에 대응하는 `internal/app`(mygo 화면·트레이)과 `internal/core`(코어 판단 로직, 인라인 테스트)입니다.

- **같은 데이터 레이아웃 공유**: `%LOCALAPPDATA%\HyMT\`의 `runtime\`, `models\`, `settings.json`을 Rust 버전과 그대로 공유하므로, 한쪽에서 런타임·모델·설정을 준비하면 다른쪽에서도 바로 동작합니다.
- **에디터**: mygo 자체 위젯을 쓰므로 GPUI 텍스트 에디터 직접 구현(`crates/hymt-app/src/text_area.rs`)이 필요 없습니다.
- **실행 파일**: `offline-translator.exe` (Windows GUI 서브시스템)

소스 빌드:

```powershell
go vet ./...
go test ./...                       # llama-server가 PATH에 있으면 서버 탐색 테스트만 실패함
go build -trimpath -ldflags "-s -w -H windowsgui" -o dist\offline-translator.exe .
```

Go 1.27+ (Windows 11 x64 기준)이면 빌드 도구 추가 설치는 필요 없습니다.

## 라이선스 및 출처

- 모델: Tencent **Hy-MT2** (Apache-2.0) — <https://huggingface.co/collections/tencent/hy-mt2>
- 추론: [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT)
- UI: [GPUI](https://gpui.rs) (Apache-2.0), mygo 포트는 [mygo](https://github.com/egoist/mygo)
- 이 애플리케이션 코드: MIT OR Apache-2.0
