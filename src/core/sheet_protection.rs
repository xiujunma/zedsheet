//! Sheet protection metadata (Phase 1.3).
//!
//! `SheetProtection` is UI-side metadata that lets the user lock a
//! sheet behind an optional password without going through the
//! `setSheetReadOnly` JS API. The data-layer "block edits" effect is
//! provided by the existing `DataProxy.read_only` flag (issue #24) —
//! `protection.enabled` is the UI affordance for it.
//!
//! **Threat model.** This is a client-side wasm app, so a determined
//! user can read the hash out of localStorage and brute-force it.
//! The password is a UX courtesy ("are you sure you want to unlock
//! this?"), not a security boundary. A simple djb2 hash is
//! sufficient — same constant-time-ish, deterministic, and tiny.

use serde::{Deserialize, Serialize};

/// Sheet-level protection metadata.
///
/// `enabled` is the only flag the data layer reads (via
/// [`DataProxy::is_read_only`] + `protection.enabled` together).
/// `password_hash` is the optional 8-char lowercase hex of a djb2
/// hash with a fixed salt prefix — `None` means no password is set,
/// so anyone can disable the protection.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SheetProtection {
    /// True while the sheet is locked.
    pub enabled: bool,
    /// Hash of the password required to disable protection. `None`
    /// means the sheet is protected without a password.
    #[serde(default)]
    pub password_hash: Option<String>,
}

impl SheetProtection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hash the password with a fixed salt prefix using djb2. Returns
    /// an 8-character lowercase hex string. The salt is deliberately
    /// fixed (and constant) — see the module-level note on the
    /// threat model.
    pub fn hash_password(password: &str) -> String {
        let salted = format!("zedsheet:protect:{}", password);
        let mut h: u32 = 5381;
        for c in salted.chars() {
            // Standard djb2: `h * 33 + c`. We use wrapping arithmetic
            // so a long password can't overflow into a panic.
            h = h.wrapping_mul(33).wrapping_add(c as u32);
        }
        format!("{:08x}", h)
    }

    /// Verify `password` against the stored hash. When no password
    /// is set (`password_hash == None`), every password "verifies"
    /// — the protection toggle just needs the enable flag cleared.
    pub fn verify(&self, password: &str) -> bool {
        match &self.password_hash {
            None => true,
            Some(h) => h == &Self::hash_password(password),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_unprotected_no_password() {
        let p = SheetProtection::default();
        assert!(!p.enabled);
        assert_eq!(p.password_hash, None);
    }

    #[test]
    fn hash_is_deterministic() {
        // Same input → same hash. Critical so a verify after a
        // protect call works across page reloads.
        assert_eq!(
            SheetProtection::hash_password("hunter2"),
            SheetProtection::hash_password("hunter2"),
        );
    }

    #[test]
    fn hash_is_different_for_different_passwords() {
        assert_ne!(
            SheetProtection::hash_password("hunter2"),
            SheetProtection::hash_password("hunter3"),
        );
    }

    #[test]
    fn hash_is_eight_hex_chars() {
        let h = SheetProtection::hash_password("anything");
        assert_eq!(h.len(), 8);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn hash_handles_empty_password() {
        // Empty password is allowed and hashes deterministically.
        assert_eq!(
            SheetProtection::hash_password(""),
            SheetProtection::hash_password(""),
        );
    }

    #[test]
    fn hash_handles_unicode_password() {
        // Multi-byte chars: each char contributes its scalar value.
        // We only require determinism + different outputs for different
        // inputs — the exact byte sequence isn't part of the contract.
        assert_eq!(
            SheetProtection::hash_password("密码"),
            SheetProtection::hash_password("密码"),
        );
        assert_ne!(
            SheetProtection::hash_password("密码"),
            SheetProtection::hash_password("密码x"),
        );
    }

    #[test]
    fn verify_with_no_password_always_true() {
        let p = SheetProtection {
            enabled: true,
            password_hash: None,
        };
        assert!(p.verify("anything"));
        assert!(p.verify(""));
        assert!(p.verify("with spaces and 1nput"));
    }

    #[test]
    fn verify_with_correct_password_returns_true() {
        let mut p = SheetProtection::default();
        p.password_hash = Some(SheetProtection::hash_password("secret"));
        assert!(p.verify("secret"));
    }

    #[test]
    fn verify_with_wrong_password_returns_false() {
        let mut p = SheetProtection::default();
        p.password_hash = Some(SheetProtection::hash_password("secret"));
        assert!(!p.verify("Secret")); // case-sensitive
        assert!(!p.verify("secre"));
        assert!(!p.verify("secret ")); // trailing space
        assert!(!p.verify(""));
    }

    #[test]
    fn serde_round_trip_with_password() {
        let p = SheetProtection {
            enabled: true,
            password_hash: Some(SheetProtection::hash_password("hunter2")),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: SheetProtection = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn serde_round_trip_without_password() {
        let p = SheetProtection {
            enabled: false,
            password_hash: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: SheetProtection = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn serde_default_password_hash_when_missing() {
        // Old workbooks without the password_hash key still load.
        let json = r#"{"enabled": true}"#;
        let p: SheetProtection = serde_json::from_str(json).unwrap();
        assert!(p.enabled);
        assert_eq!(p.password_hash, None);
    }
}
