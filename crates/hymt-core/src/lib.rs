//! Platform independent heart of HyMT, an offline translator for Windows 11 built on
//! Tencent's [Hy-MT2](https://huggingface.co/collections/tencent/hy-mt2) translation models.
//!
//! The crate deliberately has no UI, networking or OS specific dependencies: it only uses
//! `serde` for persistence and the standard library for HTTP, process control and file IO.
//! That keeps every interesting decision (prompt construction, stream parsing, process
//! supervision, settings validation) testable without a GPU, a model or a window.

pub mod client;
pub mod engine;
pub mod glossary;
pub mod http;
pub mod lang;
pub mod models;
pub mod paths;
pub mod prompt;
pub mod settings;
pub mod sse;

#[cfg(test)]
mod testutil;

pub use lang::{Language, LANGUAGES};
pub use prompt::{build_prompt, PromptLanguage, PromptMode};
pub use settings::Settings;

/// User visible product name.
pub const APP_NAME: &str = "HyMT";
/// Version of the application, shown in the UI and written to logs.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The model family this application is built for.
pub const MODEL_FAMILY: &str = "Tencent Hy-MT2";

/// Normalizes raw model output into the text that should be shown to the user.
///
/// Hy-MT2 is instructed to answer with the translation only. In practice a model can still
/// leak its chat-template control tokens (for example `<｜hy_place▁holder▁no▁2｜>`) when it
/// hits the generation limit, or wrap the answer in quotes. Those are removed here so the
/// UI never shows plumbing.
pub fn normalize_output(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n");
    let text = strip_special_tokens(&text);
    text.trim().to_string()
}

/// Removes chat-template control tokens while leaving ordinary markup such as `<div>` intact.
///
/// A token is only stripped when its body contains one of the characters Tencent's templates
/// use for control tokens (`｜`, `▁`) or when it is one of the known `<|...|>` markers. That
/// keeps HTML, XML and comparison operators untouched.
pub fn strip_special_tokens(input: &str) -> String {
    const KNOWN_INNER: &[&str] = &[
        "startoftext",
        "endoftext",
        "eos",
        "bos",
        "extra_",
        "im_start",
        "im_end",
        "endoftext",
    ];

    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // A control token is short; look ahead for a terminating '>' in a bounded window.
            let window_end = (i + 96).min(bytes.len());
            if let Some(close_rel) = input[i..window_end].find('>') {
                let close = i + close_rel;
                let inner = &input[i + 1..close];
                let looks_like_token = inner.contains('\u{FF5C}')
                    || inner.contains('\u{2581}')
                    || inner
                        .strip_prefix('|')
                        .and_then(|rest| rest.strip_suffix('|'))
                        .is_some_and(|name| {
                            let name = name.to_ascii_lowercase();
                            KNOWN_INNER.iter().any(|known| name.starts_with(known))
                        });
                if looks_like_token {
                    i = close + 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().expect("valid char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_normalizes_newlines() {
        assert_eq!(normalize_output("  안녕하세요\r\n"), "안녕하세요");
        assert_eq!(normalize_output("a\r\nb"), "a\nb");
    }

    #[test]
    fn strips_hunyuan_control_tokens() {
        let raw = "안녕하세요<｜hy_place▁holder▁no▁2｜>";
        assert_eq!(normalize_output(raw), "안녕하세요");
        assert_eq!(
            strip_special_tokens("<|startoftext|>hello<|extra_0|>"),
            "hello"
        );
    }

    #[test]
    fn keeps_real_markup() {
        let text = "<div>번역</div> <b>굵게</b> 1 < 2";
        assert_eq!(strip_special_tokens(text), text);
    }
}
