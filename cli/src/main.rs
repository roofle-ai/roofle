//! roofle-cli — headless spike binary.
//!
//! S1: workspace scaffold only. This binary just reports its version and the
//! next milestone; the real mic capture / ASR pipeline lands in S3–S7.

fn main() {
    println!(
        "roofle-cli {} — Roofle headless transcription spike",
        roofle_core::VERSION
    );
    println!("Next slice (S3): mic capture (cpal) with live RMS levels.");
    println!("Then (S6): mic -> processing -> sherpa-rs streaming ASR -> partial/final segments.");
}
