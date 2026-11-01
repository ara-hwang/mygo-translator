// Package core is the platform-independent heart of Offline Translator
// (previously HyMT), an offline translator built on Tencent's Hy-MT2 GGUF
// models run through llama.cpp.
//
// It must not depend on a UI framework: prompts, settings validation, the
// llama-server client and process supervision are all decided and tested here.
package core

// Version is shown in the UI and written to logs.
const Version = "0.1.0"

// ModelFamily names the models this application is built for.
const ModelFamily = "Tencent Hy-MT2"
