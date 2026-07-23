//! Browser emulation profiles.
//!
//! Profiles are **not** hardcoded. `wreq-util`'s `Profile` enum derives serde
//! (feature `emulation-serde`) and exposes `Profile::VARIANTS`, so we parse a
//! profile name with serde and enumerate every profile from the variant list.
//! Upgrading `wreq-util` brings new browsers for free — no code changes here.

use ext_php_rs::prelude::*;
use serde_json::Value;
use wreq_util::{Emulation as WreqEmulation, Platform, Profile};

/// Returns the canonical serde name of a profile, e.g. `"chrome_131"`.
fn profile_name(profile: &Profile) -> String {
    match serde_json::to_value(profile) {
        Ok(Value::String(s)) => s,
        _ => format!("{profile:?}"),
    }
}

/// Picks a random profile from a slice of candidates.
fn pick_random(candidates: &[&Profile]) -> Option<Profile> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    if candidates.is_empty() {
        return None;
    }
    // `RandomState` is randomly seeded per construction — enough entropy for a
    // "pick a random browser" convenience helper.
    let seed = RandomState::new().build_hasher().finish() as usize;
    Some(*candidates[seed % candidates.len()])
}

/// Returns the variants whose canonical name starts with `family_` (e.g.
/// `chrome` matches `chrome_131`, `chrome_140`, …). Family matching is
/// case-insensitive.
fn variants_for_family(family: &str) -> Vec<&'static Profile> {
    let prefix = format!("{}_", family.trim().to_lowercase());
    Profile::VARIANTS
        .iter()
        .filter(|profile| profile_name(profile).starts_with(&prefix))
        .collect()
}

/// Parses a profile name into a `wreq-util` `Profile`.
///
/// Accepts the canonical name (`chrome_131`) and tolerates common variants
/// (`Chrome131`, `chrome-131`, `chrome 131`).
pub fn parse_profile(input: &str) -> Result<Profile, String> {
    for candidate in name_candidates(input) {
        if let Ok(profile) = serde_json::from_value::<Profile>(Value::String(candidate)) {
            return Ok(profile);
        }
    }
    Err(format!("unknown emulation profile: '{input}'"))
}

/// Parses an OS name (`windows`, `macos`, `linux`, `android`, `ios`).
pub fn parse_platform(input: &str) -> Result<Platform, String> {
    for candidate in name_candidates(input) {
        if let Ok(platform) = serde_json::from_value::<Platform>(Value::String(candidate)) {
            return Ok(platform);
        }
    }
    Err(format!("unknown emulation OS: '{input}'"))
}

/// Generates name spellings to try, from strictest to most lenient.
fn name_candidates(input: &str) -> Vec<String> {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();

    // Normalize separators and insert an underscore at letter→digit boundaries
    // so `Chrome131` becomes `chrome_131`.
    let mut normalized = String::with_capacity(lower.len() + 2);
    let mut prev: Option<char> = None;
    for ch in lower.chars() {
        let ch = if ch == '-' || ch == ' ' { '_' } else { ch };
        if let Some(p) = prev {
            if p.is_ascii_alphabetic() && ch.is_ascii_digit() {
                normalized.push('_');
            }
        }
        normalized.push(ch);
        prev = Some(ch);
    }

    let mut out = vec![trimmed.to_string(), lower];
    if !out.contains(&normalized) {
        out.push(normalized);
    }
    out
}

/// `EmulationFactory`-ready value built from PHP-side options.
pub enum EmulationConfig {
    /// Just a browser profile, default OS and full fingerprint.
    Plain(Profile),
    /// Profile plus OS / fingerprint toggles.
    Detailed(WreqEmulation),
}

impl EmulationConfig {
    /// Builds from a plain profile name.
    pub fn from_name(name: &str) -> Result<Self, String> {
        Ok(Self::Plain(parse_profile(name)?))
    }

    /// Builds from a profile name and optional OS / toggles.
    pub fn detailed(
        name: &str,
        os: Option<&str>,
        skip_http2: bool,
        skip_headers: bool,
    ) -> Result<Self, String> {
        let profile = parse_profile(name)?;
        let platform = match os {
            Some(os) => parse_platform(os)?,
            None => Platform::default(),
        };
        let emulation = WreqEmulation::builder()
            .profile(profile)
            .platform(platform)
            .http2(!skip_http2)
            .headers(!skip_headers)
            .build();
        Ok(Self::Detailed(emulation))
    }

    /// Applies the emulation onto a `wreq` client builder.
    pub fn apply(self, builder: wreq::ClientBuilder) -> wreq::ClientBuilder {
        match self {
            EmulationConfig::Plain(p) => builder.emulation(p),
            EmulationConfig::Detailed(e) => builder.emulation(e),
        }
    }
}

/// Static registry of available emulation profiles, exposed to PHP.
#[php_class]
#[php(name = "Wreq\\Ext\\Emulation")]
pub struct Emulation;

#[php_impl]
impl Emulation {
    /// Every supported profile name (e.g. `chrome_131`, `firefox_136`).
    pub fn all() -> Vec<String> {
        Profile::VARIANTS.iter().map(profile_name).collect()
    }

    /// Picks a random profile, optionally restricted to a browser family
    /// (`chrome`, `firefox`, `safari`, `opera`, `edge`, `okhttp`, …). Throws
    /// when the family has no profiles, so a typo is loud instead of silently
    /// returning the same value as `random()`.
    pub fn random(family: Option<&str>) -> PhpResult<String> {
        let candidates = match family {
            Some(family) => variants_for_family(family),
            None => Profile::VARIANTS.iter().collect(),
        };
        let pick = pick_random(&candidates).ok_or_else(|| {
            ext_php_rs::exception::PhpException::default(format!(
                "no emulation profile matches family '{}'",
                family.unwrap_or(""),
            ))
        })?;
        Ok(profile_name(&pick))
    }

    /// All profile names that belong to a given browser family, in the order
    /// `wreq-util` declares them. Returns an empty list for an unknown family.
    pub fn like(family: &str) -> Vec<String> {
        variants_for_family(family)
            .iter()
            .map(|p| profile_name(p))
            .collect()
    }

    /// Whether the given profile name is recognized.
    pub fn exists(name: &str) -> bool {
        parse_profile(name).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profile_accepts_canonical_name() {
        assert!(parse_profile("chrome_131").is_ok());
    }

    #[test]
    fn parse_profile_tolerates_common_misspellings() {
        assert!(parse_profile("Chrome131").is_ok());
        assert!(parse_profile("chrome-131").is_ok());
        assert!(parse_profile("chrome 131").is_ok());
        assert!(parse_profile("  CHROME_131  ").is_ok());
    }

    #[test]
    fn parse_profile_rejects_unknown() {
        let err = parse_profile("not_a_browser").unwrap_err();
        assert!(err.contains("not_a_browser"));
    }

    #[test]
    fn parse_platform_accepts_known_values() {
        for os in ["windows", "macos", "linux", "android", "ios"] {
            assert!(parse_platform(os).is_ok(), "{os} should parse");
        }
    }

    #[test]
    fn parse_platform_rejects_garbage() {
        assert!(parse_platform("plan9").is_err());
    }

    #[test]
    fn variants_for_family_filters_by_prefix() {
        let chromes = variants_for_family("chrome");
        assert!(!chromes.is_empty(), "expected at least one chrome variant");
        for profile in &chromes {
            assert!(
                profile_name(profile).starts_with("chrome_"),
                "{:?} does not start with chrome_",
                profile_name(profile),
            );
        }
    }

    #[test]
    fn variants_for_family_is_case_insensitive() {
        assert_eq!(
            variants_for_family("Chrome").len(),
            variants_for_family("chrome").len(),
        );
    }

    #[test]
    fn variants_for_family_unknown_family_is_empty() {
        assert!(variants_for_family("nopebrowser").is_empty());
    }

    #[test]
    fn name_candidates_inserts_underscore_at_alpha_digit_boundary() {
        let cands = name_candidates("Chrome131");
        assert!(cands.iter().any(|c| c == "chrome_131"));
    }
}
