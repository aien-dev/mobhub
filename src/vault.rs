//! Torn credential resolver with explicit source metadata.
//! Credentials are held in memory here; this module does not inspect disk storage.

use std::process::Command;
use std::sync::LazyLock;

static CACHED_TORN_KEY: LazyLock<Option<ResolvedSecret>> =
    LazyLock::new(|| resolve_secret_with_source("TORN_API_KEY"));
static KEY_PARAM_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"key=[^&\s]+").expect("Failed to compile KEY_PARAM_RE regex")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSource {
    AtlasVault,
    Environment,
}

#[derive(Clone)]
pub struct ResolvedSecret {
    pub value: String,
    pub source: SecretSource,
}

pub fn resolve_secret_with_source(key_name: &str) -> Option<ResolvedSecret> {
    if let Ok(out) = Command::new("atlas-vault").args(["get", key_name]).output() {
        if out.status.success() {
            let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !val.is_empty() {
                return Some(ResolvedSecret {
                    value: val,
                    source: SecretSource::AtlasVault,
                });
            }
        }
    }
    std::env::var(key_name).ok().and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(ResolvedSecret {
            value,
            source: SecretSource::Environment,
        })
    })
}

/// Dynamically resolve a secret token.
/// Checks atlas-vault first, then the process environment. This does not certify TPM binding.
pub fn resolve_secret(key_name: &str) -> Option<String> {
    resolve_secret_with_source(key_name).map(|secret| secret.value)
}

/// Retrieve the Torn API Key dynamically into memory.
pub fn get_torn_api_key() -> Option<String> {
    CACHED_TORN_KEY.as_ref().map(|secret| secret.value.clone())
}

pub fn torn_key_source() -> Option<SecretSource> {
    CACHED_TORN_KEY.as_ref().map(|secret| secret.source)
}

/// Actively redact vault secrets and API key signatures from log streams.
pub fn redact_secrets(input: &str) -> String {
    let mut sanitized = input.to_string();
    if let Some(key) = get_torn_api_key() {
        if !key.is_empty() && key.len() >= 6 {
            sanitized = sanitized.replace(&key, "[REDACTED]");
        }
    }

    // Actively redact all key=query parameters across the entire input stream
    KEY_PARAM_RE
        .replace_all(&sanitized, "key=[REDACTED]")
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
        assert_eq!(redacted.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn environment_key_is_not_reported_as_hardware_vault_key() {
        const NAME: &str = "MOBHUB_VAULT_SOURCE_TEST_ONLY_20261026";
        std::env::set_var(NAME, "from-environment");
        let resolved = resolve_secret_with_source(NAME).unwrap();
        std::env::remove_var(NAME);
        assert_eq!(resolved.source, SecretSource::Environment);
        assert_eq!(resolved.value, "from-environment");
    }
}
