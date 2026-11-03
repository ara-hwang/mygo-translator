# Offline Translator — Windows 11용 오프라인 번역기 (mygo + Tencent Hy-MT2)

mygo(Go 네이티브 UI 라이브러리)로 만든 데스크톱 번역기입니다. 추론은 Tencent의 최신 번역 모델 계열인 **Hy-MT2**(1.8B / 7B / 30B-A3B)를 llama.cpp로 로컬에서 실행하며, 인터넷 연결 없이 동작합니다. 네트워크는 모델 파일을 한 번 내려받을 때만 사용합니다.

```
┌─ Offline Translator ───────────────────────────────────────────────┐
│  번역 | 모델 | 설정                                                 │
│  언어 [한국어 ▾] → [영어 ▾]                    A− 14 px  A+         │
│  ┌ 원문 ─────────────┐  ┌──────────── 번역 ───────┐                 │
│  │ GPUI is fast.     │  │ GPUI는 빠릅니다.        │                 │
│  └──────────────────┘  └─────────────────────────┘                 │
│  ● 엔진 준비됨                                        [번역]       │
└────────────────────────────────────────────────────────────────────┘
```

## 주요 기능

- **완전 오프라인 추론**: 모델 서버는 `127.0.0.1`에만 바인딩되며 외부로 나가는 요청이 없습니다.
- **스트리밍 번역**: 토큰이 생성되는 즉시 화면에 표시되고, 중단할 수 있습니다.
- **단축키 선택 번역**: 다른 앱에서 텍스트를 선택한 상태로 단축키(기본 `Ctrl+Shift+C`)를 누르면 창이 열리면서 바로 번역합니다. 설정 탭에서 켤 수 있습니다. 선택 텍스트는 현재 선택된 언어 쌍으로 번역합니다.
- **원문/번역 나란히 보기**: 번역 탭의 원문과 번역 패널이 옆으로 나란히 배치되고, 가운데 분할 막대를 드래그해 폭을 조절합니다.
- **Hy-MT2 공식 프롬프트 4종**: 기본 번역, **용어집(terminology intervention)**, **스타일 지정**, **구분자 보존**. 지시문 언어(자동/영어/중국어)도 선택할 수 있습니다.
- **38개 언어**: 모델 카드에 명시된 33개 언어 + 번체 중국어, 광둥어, 티베트어, 카자흐어, 몽골어, 위구르어.
- **모델 관리 내장**: Tencent 공식 GGUF 저장소에서 검증된 파일 크기와 함께 다운로드(이어받기 지원), 로컬 `.gguf` 자동 스캔.
- **엔진 감독**: llama.cpp 서버 프로세스를 앱이 직접 관리하고, `/health`가 준비될 때까지 상태를 보여 줍니다. 크래시해도 UI는 살아 있습니다.
- **트레이·자동 실행**: 설정에서 창을 닫아도 트레이 아이콘으로 유지하거나 Windows 시작 시 자동 실행할 수 있습니다.
- **이식성**: 포터블 배포 시 exe 옆 폴더에 데이터가 함께 있습니다(저장 위치 참고).

## 요구 사항 (Windows 11)

| 항목 | 요구 사항 |
| --- | --- |
| OS | Windows 11 x64 |
| Go | 소스 빌드 시 1.27 이상 (`go.mod` 기준), 빌드 도구 추가 설치 불필요 |
| 런타임 | `llama-server.exe` (llama.cpp Windows 빌드) — 앱에 포함되지 않습니다 |
| 디스크 | 모델 1.06GB(1.8B Q4) ~ 8GB(7B Q8) + 런타임 약 100MB |
| 메모리 | 1.8B Q4 기준 3GB 이상, 7B Q4 기준 8GB 이상 권장 |

## 빠른 시작

1. **앱 실행**
   GitHub Releases의 설치 마법사(`-setup.exe`) 또는 휴대용 ZIP을 사용하거나, 소스에서 빌드합니다.
   ```powershell
   git clone <this-repo> offline-translator
   cd offline-translator
   go build -trimpath -ldflags "-s -w -H windowsgui" -o dist\offline-translator.exe .
   ```

2. **llama.cpp 런타임 준비**
   [llama.cpp Releases](https://github.com/ggml-org/llama.cpp/releases)에서 Windows용 zip을 받아 압축을 풀고, `llama-server.exe`와 같은 폴더의 DLL들을 `%LOCALAPPDATA%\HyMT\runtime\`에 넣습니다.
   자동으로 받으려면:
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\setup-runtime.ps1 -Variant cpu
   ```
   NVIDIA GPU라면 `-Variant cuda`, Vulkan은 `-Variant vulkan`을 사용합니다.

3. **모델 다운로드**
   앱 실행 → **모델** 탭 → `Hy-MT2-1.8B-Q4_K_M.gguf`(권장)의 **설치됨** 여부 확인, 없으면 내려받기. 다운로드가 끝나면 엔진을 시작합니다.
   사내망 등으로 다운로드가 안 되면 `.gguf` 파일을 직접 `%LOCALAPPDATA%\HyMT\models\`에 넣으세요.

4. **번역**
   원문을 입력하고 **번역** 버튼을 누릅니다. 번역 탭의 글자 크기(`A−` / `A+`)로 양쪽 에디터 크기를 조절할 수 있습니다.
   다른 앱의 선택 텍스트를 번역하려면 **설정 탭**에서 **선택한 텍스트를 자동 번역**을 켜세요(기본 단축키 `Ctrl+Shift+C`).

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

## 단축키로 선택 텍스트 번역

트리거가 발동하면:

1. 포커스된 앱의 **선택 텍스트**를 읽습니다. 대부분의 앱은 합성 `Ctrl+C`로 복사해 클립보드에서 읽습니다(원래 클립보드 내용은 복사 후 복원됩니다). 합성 복사를 위해 입력 중인 수정키(Ctrl·Shift·Alt·Win)를 논리적으로 뗐다가 되돌리므로, `Ctrl+Shift+C`를 누르고 있어도 복사가 막히지 않습니다.
2. 창을 띄우고 원문에 넣어 **바로 번역**합니다. 언어 쌍은 주 창에 선택된 값을 그대로 사용합니다.
3. UI Automation(Chrome·Word 같은 직접 읽기)은 지원하지 않고, 항상 클립보드 방식입니다.

다른 프로그램이 같은 조합을 이미 쓰면 등록이 실패하고, 상태는 앱 하단 알림으로 표시됩니다. 찾는 앱이 관리자 권한으로 실행 중이면 보안(UIPI) 때문에 합성 Ctrl+C가 실패할 수 있습니다.

## 저장 위치

- 기본: `%LOCALAPPDATA%\HyMT\` (`settings.json`, `models\`, `runtime\`, `logs\`)
- 포터블 모드: exe 옆에 `models` 폴더가 있으면 exe 폴더를 사용합니다.

`settings.json`에는 언어 쌍, 생성 파라미터(기본값은 Tencent 권장값: temperature 0.7, top_p 0.6, top_k 20, repetition_penalty 1.05, max_tokens 4096), 프롬프트 옵션, 서버 옵션(ctx, threads, GPU layers, port), 트레이·자동 실행 옵션, 단축키 옵션, 에디터 글자 크기가 저장됩니다. 파일 레이아웃은 과거 버전과 호환됩니다.

## 문제 해결

| 증상 | 해결 |
| --- | --- |
| "사용할 모델이 없습니다." | 모델 탭에서 GGUF를 내려받거나 `%LOCALAPPDATA%\HyMT\models\`에 넣기 |
| "llama-server를 찾을 수 없습니다" | `%LOCALAPPDATA%\HyMT\runtime\llama-server.exe` 존재 여부 확인, 또는 `scripts\setup-runtime.ps1` 실행 |
| 포트 충돌 | `settings.json`의 `server.port`를 다른 값으로 변경 후 엔진 재시작 |
| GPU를 쓰고 싶다 | 설정에서 GPU layers를 99(전부)나 -1(자동)로. CUDA/Vulkan 빌드의 llama-server 필요 |
| 번역이 느림 | threads를 물리 코어 수에 맞추고, 1.8B Q4 모델 사용 |
| 단축키를 눌렀는데 아무 동작이 없음 | 설정 탭에서 스위치가 켜져 있는지 확인. 대상 앱이 관리자 권한이면 UIPI 때문에 합성 Ctrl+C가 실패합니다. 앱을 일반 권한으로 실행하거나 텍스트를 직접 복사해 사용하세요 |

## 구조

```
main.go             진입점 (app.Run)
internal/
  app/              mygo 화면·트레이·상태 연결
    app.go          앱 상태·엔진 감독·스트리밍·다운로드·전역 단축키
    ui.go           번역/모델/설정 탭
    assets.go       아이콘 embed (`internal/app/assets`)
  core/             플랫폼 독립 코어 (표준 라이브러리 + x/sys만)
    lang.go         38개 언어표
    prompt.go       Hy-MT2 공식 프롬프트 템플릿
    server.go       OpenAI 호환 스트리밍 번역 클라이언트, /health 대기
    engine.go       llama-server 탐색·실행 인자·프로세스 감독
    models.go       공식 GGUF 카탈로그·다운로드·진행률
    settings.go     설정 검증·원자적 저장
    paths.go        설치/포터블 경로 해석
    glossary.go     용어집 파서
    normalize.go    출력 정규화
  winapi/           mygo가 감싸지 않는 Win32 (선택 텍스트 복사 합성, 클립보드)
scripts/
  setup-runtime.ps1 llama.cpp 런타임 내려받기
  installer.iss     Inno Setup 설치 마법사 스크립트
```

### 검증 상태

- `go test ./...`: 코어(프롬프트, 언어표, SSE 청크 분할, 취소, 타임아웃, 헬스 대기, 모델 선택, 설정 검증(단축키 포함), 경로, 용어집, 출력 정규화) 통과. PATH에 llama-server가 있으면 서버 탐색 테스트만 실패합니다(상단 참고).
- `gofmt -l .` / `go vet ./...` 통과.
- 단축키의 실제 등록·트리거·캡처 동작과 UI 조작은 Windows 전용이라 이 저장소의 자동화 범위 밖입니다. Windows에서 설정 탭 → 단축키 스위치로 직접 확인하세요.

## 라이선스 및 출처

- 모델: Tencent **Hy-MT2** (Apache-2.0) — <https://huggingface.co/collections/tencent/hy-mt2>
- 추론: [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT)
- UI: [mygo](https://github.com/egoist/mygo)
- 이 애플리케이션 코드: MIT OR Apache-2.0
