//! Token counting via tiktoken (`tiktoken-rs`). Default encoding is
//! `o200k_base` (GPT-4o / o-series); `cl100k_base` is also supported.
//!
//! A single `TokenCounter` is built once per scan and shared across worker
//! threads (`CoreBPE` is immutable and `Sync`).

use anyhow::{Result, anyhow};
use tiktoken_rs::{CoreBPE, cl100k_base, o200k_base};

pub struct TokenCounter {
    bpe: CoreBPE,
    name: String,
}

impl TokenCounter {
    /// Build a counter for a supported canonical encoding or alias.
    ///
    /// # Errors
    ///
    /// Returns an error when `encoding` is unknown or the selected tokenizer
    /// vocabulary cannot be initialized.
    pub fn new(encoding: &str) -> Result<Self> {
        let (bpe, name) = match encoding.to_ascii_lowercase().as_str() {
            "o200k_base" | "o200k" => (o200k_base()?, "o200k_base"),
            "cl100k_base" | "cl100k" => (cl100k_base()?, "cl100k_base"),
            other => {
                return Err(anyhow!(
                    "unknown encoding '{other}' (supported: o200k_base, cl100k_base)"
                ));
            }
        };
        Ok(Self {
            bpe,
            name: name.to_string(),
        })
    }

    /// Canonical encoding name (e.g. "`o200k_base`").
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Count tokens in `text`, ignoring special-token semantics so arbitrary
    /// source code never errors.
    #[must_use]
    pub fn count(&self, text: &str) -> usize {
        self.bpe.encode_ordinary(text).len()
    }
}

#[cfg(test)]
mod tests {
    use super::TokenCounter;

    // Golden counts published by OpenAI's "How to count tokens with tiktoken"
    // cookbook example. These catch asset or port drift in both supported BPEs.
    const VECTORS: &[(&str, usize, usize)] = &[
        ("antidisestablishmentarianism", 6, 6),
        ("2 + 2 = 4", 7, 7),
        (
            "\u{304a}\u{8a95}\u{751f}\u{65e5}\u{304a}\u{3081}\u{3067}\u{3068}\u{3046}",
            9,
            8,
        ),
    ];

    #[test]
    fn supported_encodings_match_openai_golden_vectors() {
        let cl100k = TokenCounter::new("cl100k_base").expect("cl100k");
        let o200k = TokenCounter::new("o200k_base").expect("o200k");

        for (text, expected_cl100k, expected_o200k) in VECTORS {
            assert_eq!(cl100k.count(text), *expected_cl100k, "cl100k: {text}");
            assert_eq!(o200k.count(text), *expected_o200k, "o200k: {text}");
        }
    }
}
