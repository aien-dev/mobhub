//! Hardware TPM Key Vault client and memory security layer.
//! Guarantees zero plaintext secret files on disk.

use std::process::Command;
use std::sync::OnceLock;

static CACHED_TORN_KEY: OnceLock<Option<String>> = OnceLock::new();

/// Dynamically resolve a secret token.
/// Checks hardware TPM vault first (atlas-vault get <KEY>), then falls back to in-memory environment.
pub fn resolve_secret(key_name: &str) -> Option<String> {
    // 1. Try hardware TPM-bound vault on NVIDIA DGX Spark
    if let Ok(out) = Command::new("atlas-vault").args(["get", key_name]).output() {
        if out.status.success() {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }

    // 2. Fallback to process environment (e.g. for CI or local overrides)
    if let Ok(env_val) = std::env::var(key_name) {
        let trimmed = env_val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    None
}

/// Retrieve the Torn API Key dynamically into memory.
pub fn get_torn_api_key() -> Option<String> {
    CACHED_TORN_KEY
        .get_or_init(|| resolve_secret("TORN_API_KEY"))
        .clone()
}

/// Verify if the Torn API Key is present in the vault or memory.
pub fn is_torn_key_present() -> bool {
    get_torn_api_key().is_some()
}

/// Actively redact vault secrets and API key signatures from log streams.
pub fn redact_secrets(input: &str) -> String {
    let mut sanitized = input.to_string();
    if let Some(key) = get_torn_api_key() {
        if !key.is_empty() && key.len() >= 6 {
            sanitized = sanitized.replace(&key, "[REDACTED_BY_ATLAS_VAULT]");
        }
    }
    // Also redact any potential query param pattern like key=XXXXX
    let re_pattern = "key=";
    if let Some(idx) = sanitized.find(re_pattern) {
        let after = &sanitized[idx + 4..];
        let end = after.find('&').unwrap_or(after.len());
        let secret_slice = &after[..end];
        if !secret_slice.is_empty() {
            sanitized = sanitized.replace(
                &format!("key={}", secret_slice),
                "key=[REDACTED_BY_ATLAS_VAULT]",
            );
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_redaction() {
        let raw = "GET https://api.torn.com/v2/faction/rankedwars?key=abcdef123456&limit=10";
        let redacted = redact_secrets(raw);
        assert!(!redacted.contains("abcdef123456"));
        assert!(redacted.contains("[REDACTED_BY_ATLAS_VAULT]"));
    }
}
