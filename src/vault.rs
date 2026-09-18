//! Hardware TPM Key Vault client and memory security layer.
//! Guarantees zero plaintext secret files on disk.

use std::process::Command;
use std::sync::{LazyLock, OnceLock};

static CACHED_TORN_KEY: OnceLock<Option<String>> = OnceLock::new();
static KEY_PARAM_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"key=[^&\s]+").expect("Failed to compile KEY_PARAM_RE regex")
});

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

    // Actively redact all key=query parameters across the entire input stream
    KEY_PARAM_RE
        .replace_all(&sanitized, "key=[REDACTED_BY_ATLAS_VAULT]")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_redaction() {
        let raw = "GET https://api.torn.com/v2/faction/rankedwars?key=abcdef123456&limit=10 and retry key=9876543210zyx";
        let redacted = redact_secrets(raw);
        assert!(!redacted.contains("abcdef123456"));
        assert!(!redacted.contains("9876543210zyx"));
        assert_eq!(redacted.matches("[REDACTED_BY_ATLAS_VAULT]").count(), 2);
    }
}
