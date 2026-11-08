//! DigiLingua clinical data model, TextGrid/summary and the AI prompt.
//! Pure Rust (no I/O threads), so it builds for the native app and for WebAssembly.
pub mod clinic;
pub mod prompt;
pub mod protocol;
