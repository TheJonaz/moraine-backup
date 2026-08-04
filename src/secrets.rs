//! Where a target's secrets come from.
//!
//! The secret fields in the config (`password`, `crypt_password`, `crypt_salt`)
//! hold a **spec** — a description of where the secret lives, which is not
//! necessarily the secret itself:
//!
//! | spec              | meaning                                                |
//! |-------------------|--------------------------------------------------------|
//! | `""`              | no secret                                              |
//! | `"env:VAR"`       | read from the environment variable `VAR`               |
//! | `"keyring:"`      | read from the OS keyring, under a key derived from the |
//! |                   | target name and field                                  |
//! | `"keyring:ACCT"`  | ditto, but under the explicit account name `ACCT`      |
//! | `"literal:SECRET"`| the literal `SECRET` (escape hatch, see below)          |
//! | anything else     | the literal secret — the historical format             |
//!
//! Bare literals still work, so every config written before this module existed
//! keeps working unchanged. They do mean the secret sits in a plaintext file,
//! which is what `moraine secrets migrate` moves away from.
//!
//! The `literal:` prefix exists for the one case the bare form can't express: a
//! real password that happens to start with `env:` or `keyring:`.
//!
//! Resolution is deliberately **fallible and loud**. A secret that silently
//! resolves to the empty string is the dangerous failure here — it turns an
//! encrypted destination into a plaintext one, or an authenticated login into a
//! mysterious permission error at 3 AM. Every caller that needs the value gets
//! a `Result` and must deal with it.

use anyhow::{bail, Result};
// Only the keyring paths add context to an underlying error; without the
// feature the trait would be an unused import.
#[cfg(feature = "keyring")]
use anyhow::Context;

/// What a spec points at. Every other function here derives from [`parse`], so
/// the scheme rules can't drift apart between resolving, migrating and
/// describing a spec.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    /// No secret configured.
    Unset,
    /// The secret is in the config file, verbatim.
    Literal(String),
    /// Read from this environment variable.
    Env(String),
    /// Read from the OS keyring under this account name.
    Keyring(String),
}

/// Classifies a spec. `target_name`/`field` only supply the derived keyring
/// account for a bare `keyring:`; nothing here touches the environment or the
/// keyring, so it is safe on paths that must not trigger an unlock prompt.
///
/// Note where trimming does and does not apply: the *scheme* is detected on a
/// trimmed spec (so TOML formatting can't break it), but a literal secret is
/// returned verbatim. A password may legitimately begin or end with a space,
/// and silently trimming one would turn a working config into an
/// authentication failure with no visible cause.
pub fn parse(spec: &str, target_name: &str, field: &str) -> Source {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Source::Unset;
    }
    // trim_start only: whatever follows the prefix is the secret, trailing
    // whitespace included.
    if let Some(rest) = spec.trim_start().strip_prefix("literal:") {
        return Source::Literal(rest.to_string());
    }
    if let Some(var) = trimmed.strip_prefix("env:") {
        return Source::Env(var.trim().to_string());
    }
    if let Some(account) = trimmed.strip_prefix("keyring:") {
        let account = account.trim();
        return Source::Keyring(if account.is_empty() {
            default_account(target_name, field)
        } else {
            account.to_string()
        });
    }
    // Historical format: the spec *is* the secret, verbatim.
    Source::Literal(spec.to_string())
}

/// Whether a spec refers to a secret at all. Cheap — see [`parse`].
pub fn is_set(spec: &str) -> bool {
    !spec.trim().is_empty()
}

/// Resolves a spec to the actual secret. `target_name` and `field` are used to
/// derive the keyring key and to make errors say which target is at fault.
pub fn resolve(spec: &str, target_name: &str, field: &str) -> Result<String> {
    match parse(spec, target_name, field) {
        Source::Unset => Ok(String::new()),
        Source::Literal(secret) => Ok(secret),
        Source::Env(var) if var.is_empty() => {
            bail!("target '{target_name}': {field} is \"env:\" with no variable name")
        }
        // Not `.context()`: std's own "environment variable not found" adds
        // nothing and reads as duplication after our sentence.
        Source::Env(var) => match std::env::var(&var) {
            Ok(v) => Ok(v),
            Err(_) => bail!("target '{target_name}': {field} reads ${var}, which is not set"),
        },
        Source::Keyring(account) => keyring_get(&account, target_name, field),
    }
}

/// The secret itself if the spec keeps it *in the config file*. `None` when the
/// spec is empty or already points elsewhere (env, keyring).
///
/// This is what `moraine secrets migrate` moves into the keyring.
pub fn literal_value(spec: &str) -> Option<String> {
    match parse(spec, "", "") {
        Source::Literal(secret) => Some(secret),
        _ => None,
    }
}

/// Renders a raw secret as a spec that resolves back to exactly it — the
/// inverse of [`resolve`] for the in-config case. Used when a config is
/// exported as a self-contained backup.
///
/// Anything that would otherwise be re-read as a scheme, or lose surrounding
/// whitespace, gets the explicit `literal:` prefix.
pub fn literal_spec(secret: &str) -> String {
    if secret.is_empty() {
        return String::new();
    }
    let needs_prefix = secret != secret.trim()
        || secret.starts_with("env:")
        || secret.starts_with("keyring:")
        || secret.starts_with("literal:");
    if needs_prefix {
        format!("literal:{secret}")
    } else {
        secret.to_string()
    }
}

/// Stores a secret in the keyring and reads it back to confirm it survived, so
/// the caller can safely drop its only other copy.
pub fn store_verified(target_name: &str, field: &str, secret: &str) -> Result<()> {
    keyring_set(target_name, field, secret)?;
    let back = resolve("keyring:", target_name, field)?;
    if back != secret {
        bail!(
            "the keyring returned a different value for {target_name}/{field} — \
             leaving the config alone"
        );
    }
    Ok(())
}

/// A human-readable description of *where* a spec reads its secret from, safe to
/// print: it never includes the secret itself, not even for a literal spec.
pub fn source_of(spec: &str) -> String {
    // The sentinel account marks a bare `keyring:`, so the description can say
    // "the keyring" rather than echoing a derived name the user never wrote.
    match parse(spec, "\u{0}", "\u{0}") {
        Source::Unset => "unset".to_string(),
        Source::Literal(_) => "the config file".to_string(),
        Source::Env(var) => format!("${var}"),
        Source::Keyring(a) if a == default_account("\u{0}", "\u{0}") => "the keyring".to_string(),
        Source::Keyring(account) => format!("the keyring entry '{account}'"),
    }
}

/// The keyring account a target's field is stored under when the spec is a bare
/// `keyring:`. Target names are already validated to be a single path-safe
/// segment (see `Config::validate`), so this can't collide across targets or be
/// steered somewhere else by a crafted name.
pub fn default_account(target_name: &str, field: &str) -> String {
    format!("{target_name}/{field}")
}

/// The keyring service all of Moraine's secrets live under.
pub const SERVICE: &str = "moraine";

/// The secret fields a target has, as they are spelled in the config. The one
/// list every tool that iterates over secrets works from, so a fourth secret
/// can't be added to `Target` and silently missed by `moraine secrets` or the
/// desktop client.
pub const FIELDS: [&str; 3] = ["password", "crypt_password", "crypt_salt"];

#[cfg(not(feature = "keyring"))]
fn keyring_get(_account: &str, target_name: &str, field: &str) -> Result<String> {
    bail!(
        "target '{target_name}': {field} is set to \"keyring:\", but this build of Moraine \
         has no keyring support compiled in. Rebuild with `--features keyring`, or point the \
         field at an environment variable instead (e.g. {field} = \"env:MORAINE_SECRET\")"
    )
}

#[cfg(feature = "keyring")]
fn keyring_get(account: &str, target_name: &str, field: &str) -> Result<String> {
    let entry = keyring::Entry::new(SERVICE, account).with_context(|| {
        format!("target '{target_name}': could not address the keyring for {field}")
    })?;
    match entry.get_password() {
        Ok(secret) => Ok(secret),
        // `secrets set` only ever writes the derived account, so suggesting it
        // for a hand-picked one would send the user in a circle.
        Err(keyring::Error::NoEntry) if account == default_account(target_name, field) => bail!(
            "target '{target_name}': {field} should be in the keyring under '{account}', but \
             there is no such entry. Store it with `moraine secrets set --target {target_name} \
             --field {field}`"
        ),
        Err(keyring::Error::NoEntry) => bail!(
            "target '{target_name}': {field} points at the keyring entry '{account}', which \
             does not exist. Create it in your keyring under the service '{SERVICE}', or use a \
             bare `{field} = \"keyring:\"` and let `moraine secrets set --target {target_name} \
             --field {field}` manage it"
        ),
        // Everything else means the keyring itself is unavailable: no Secret
        // Service running, a locked session, no D-Bus. This is the headless/cron
        // case, so the message has to name the way out rather than just the
        // underlying error.
        Err(e) => bail!(
            "target '{target_name}': could not read {field} from the keyring ({e}). On a \
             headless machine or from cron there is usually no unlocked keyring — use \
             {field} = \"env:SOME_VAR\" for those runs instead"
        ),
    }
}

/// Stores a secret in the keyring under the account `default_account` derives.
#[cfg(feature = "keyring")]
pub fn keyring_set(target_name: &str, field: &str, secret: &str) -> Result<()> {
    let account = default_account(target_name, field);
    let entry = keyring::Entry::new(SERVICE, &account)
        .with_context(|| format!("could not address the keyring entry '{account}'"))?;
    entry
        .set_password(secret)
        .with_context(|| format!("could not write the keyring entry '{account}'"))
}

#[cfg(not(feature = "keyring"))]
pub fn keyring_set(_target_name: &str, _field: &str, _secret: &str) -> Result<()> {
    bail!("this build of Moraine has no keyring support compiled in (rebuild with `--features keyring`)")
}

/// Removes a secret from the keyring. Missing entries are not an error.
#[cfg(feature = "keyring")]
pub fn keyring_delete(target_name: &str, field: &str) -> Result<()> {
    let account = default_account(target_name, field);
    let entry = keyring::Entry::new(SERVICE, &account)
        .with_context(|| format!("could not address the keyring entry '{account}'"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "could not delete the keyring entry '{account}': {e}"
        )),
    }
}

#[cfg(not(feature = "keyring"))]
pub fn keyring_delete(_target_name: &str, _field: &str) -> Result<()> {
    bail!("this build of Moraine has no keyring support compiled in (rebuild with `--features keyring`)")
}

/// Whether this build can talk to a keyring at all. Used by `secrets migrate`
/// to fail before it rewrites the config rather than after.
pub const fn keyring_supported() -> bool {
    cfg!(feature = "keyring")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_spec_resolves_to_empty() {
        assert_eq!(resolve("", "nas", "password").unwrap(), "");
        assert_eq!(resolve("   ", "nas", "password").unwrap(), "");
        assert!(!is_set(""));
        assert!(!is_set("  "));
    }

    #[test]
    fn bare_literal_is_the_secret() {
        assert_eq!(resolve("hunter2", "nas", "password").unwrap(), "hunter2");
        assert!(is_set("hunter2"));
    }

    #[test]
    fn literal_prefix_escapes_the_schemes() {
        // A real password that looks like a scheme must survive verbatim.
        assert_eq!(
            resolve("literal:env:NOTAVAR", "nas", "password").unwrap(),
            "env:NOTAVAR"
        );
        assert_eq!(
            resolve("literal:keyring:", "nas", "password").unwrap(),
            "keyring:"
        );
    }

    #[test]
    fn env_scheme_reads_the_variable() {
        std::env::set_var("MORAINE_TEST_SECRET", "from-env");
        assert_eq!(
            resolve("env:MORAINE_TEST_SECRET", "nas", "password").unwrap(),
            "from-env"
        );
        std::env::remove_var("MORAINE_TEST_SECRET");
    }

    #[test]
    fn unset_env_var_is_an_error_not_an_empty_secret() {
        let err = resolve("env:MORAINE_DEFINITELY_UNSET", "nas", "password")
            .unwrap_err()
            .to_string();
        assert!(err.contains("nas"), "error names the target: {err}");
        assert!(err.contains("MORAINE_DEFINITELY_UNSET"), "{err}");
    }

    #[test]
    fn env_scheme_without_a_name_is_rejected() {
        assert!(resolve("env:", "nas", "password").is_err());
        assert!(resolve("env:   ", "nas", "password").is_err());
    }

    /// The scheme is matched on a trimmed spec, but the secret itself must come
    /// back byte for byte. Trimming it would break a password with edge
    /// whitespace in a way that looks exactly like a wrong password.
    #[test]
    fn a_literal_secret_keeps_its_whitespace() {
        assert_eq!(
            resolve("  padded  ", "n", "password").unwrap(),
            "  padded  "
        );
        assert_eq!(resolve("literal:  x  ", "n", "password").unwrap(), "  x  ");
        // Scheme detection still tolerates formatting whitespace around it.
        std::env::set_var("MORAINE_TEST_WS", "v");
        assert_eq!(
            resolve("  env:MORAINE_TEST_WS  ", "n", "password").unwrap(),
            "v"
        );
        std::env::remove_var("MORAINE_TEST_WS");
    }

    /// `literal_spec` is the inverse of `resolve` for in-config secrets: the
    /// config export relies on every secret surviving the round trip exactly.
    #[test]
    fn literal_spec_round_trips_every_awkward_secret() {
        for secret in [
            "hunter2",
            "  leading and trailing  ",
            "env:not-a-scheme",
            "keyring:not-a-scheme",
            "literal:not-a-scheme",
            "pass word",
            "åäö 🔒",
            "",
        ] {
            let spec = literal_spec(secret);
            assert_eq!(
                resolve(&spec, "n", "password").unwrap(),
                secret,
                "round trip failed for {secret:?} (spec {spec:?})"
            );
        }
    }

    #[test]
    fn literal_value_finds_only_secrets_kept_in_the_config() {
        // These live in the config file → migrate must move them.
        assert_eq!(literal_value("hunter2").as_deref(), Some("hunter2"));
        assert_eq!(literal_value("literal:env:x").as_deref(), Some("env:x"));
        // Verbatim, not trimmed: migrate must store exactly the secret that was
        // authenticating before, or it silently changes it.
        assert_eq!(literal_value("  spaced  ").as_deref(), Some("  spaced  "));
        // These point elsewhere → migrate must leave them alone. Moving an
        // `env:` spec would silently store the *name of the variable* as the
        // secret and break every scheduled run using it.
        assert!(literal_value("").is_none());
        assert!(literal_value("   ").is_none());
        assert!(literal_value("env:MY_VAR").is_none());
        assert!(literal_value("keyring:").is_none());
        assert!(literal_value("keyring:custom").is_none());
    }

    #[test]
    fn source_of_never_reveals_the_secret() {
        assert_eq!(source_of("hunter2"), "the config file");
        assert!(!source_of("hunter2").contains("hunter2"));
        assert_eq!(source_of(""), "unset");
        assert_eq!(source_of("env:MY_VAR"), "$MY_VAR");
        assert_eq!(source_of("keyring:"), "the keyring");
        assert_eq!(source_of("keyring:acct"), "the keyring entry 'acct'");
        // A literal that looks like a scheme must not be echoed either.
        assert!(!source_of("literal:hunter2").contains("hunter2"));
    }

    #[test]
    fn keyring_account_is_derived_per_target_and_field() {
        assert_eq!(default_account("nas", "password"), "nas/password");
        assert_eq!(
            default_account("nas", "crypt_password"),
            "nas/crypt_password"
        );
    }

    /// Round-trips a real secret through the platform keyring. Ignored by
    /// default: it touches the developer's actual credential store and needs an
    /// unlocked session, so it must never run in CI. Run it by hand after
    /// changing anything in the keyring path:
    ///     cargo test --features keyring -- --ignored keyring_round_trip
    #[cfg(feature = "keyring")]
    #[test]
    #[ignore = "touches the real OS keyring; needs an unlocked desktop session"]
    fn keyring_round_trip() {
        let target = "moraine-selftest";
        keyring_set(target, "password", "hunter2").expect("store");
        let spec = "keyring:";
        assert_eq!(resolve(spec, target, "password").unwrap(), "hunter2");
        keyring_delete(target, "password").expect("delete");
        // Deleting twice is not an error, and a missing entry must be reported
        // as such rather than resolving to an empty secret.
        keyring_delete(target, "password").expect("second delete is a no-op");
        assert!(resolve(spec, target, "password").is_err());
    }

    #[cfg(not(feature = "keyring"))]
    #[test]
    fn keyring_spec_without_the_feature_explains_itself() {
        let err = resolve("keyring:", "nas", "password")
            .unwrap_err()
            .to_string();
        assert!(err.contains("keyring"), "{err}");
        // Must not silently degrade to the literal "keyring:".
        assert!(err.contains("env:"), "error offers the way out: {err}");
    }
}
