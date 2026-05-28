//! Browser emulation profiles.
//!
//! Profiles are **not** hardcoded. `wreq-util`'s `Emulation` enum derives serde
//! (feature `emulation-serde`) and `VariantArray` (feature `emulation-rand`),
//! so we parse a profile name with serde and enumerate every profile from
//! `Emulation::VARIANTS`. Upgrading `wreq-util` brings new browsers for free —
//! no code changes here.

use ext_php_rs::prelude::*;
use serde_json::Value;
use strum::VariantArray;
use wreq_util::{Emulation as WreqEmulation, EmulationOS, EmulationOption};

/// Returns the canonical serde name of a profile, e.g. `"chrome_131"`.
fn profile_name(emulation: &WreqEmulation) -> String {
    match serde_json::to_value(emulation) {
        Ok(Value::String(s)) => s,
        _ => format!("{emulation:?}"),
    }
}

/// Picks a random profile from a slice of candidates.
fn pick_random(candidates: &[&WreqEmulation]) -> Option<WreqEmulation> {
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
fn variants_for_family(family: &str) -> Vec<&'static WreqEmulation> {
    let prefix = format!("{}_", family.trim().to_lowercase());
    WreqEmulation::VARIANTS
        .iter()
        .filter(|emulation| profile_name(emulation).starts_with(&prefix))
        .collect()
}

/// Parses a profile name into a `wreq-util` `Emulation`.
///
/// Accepts the canonical name (`chrome_131`) and tolerates common variants
/// (`Chrome131`, `chrome-131`, `chrome 131`).
pub fn parse_emulation(input: &str) -> Result<WreqEmulation, String> {
    for candidate in name_candidates(input) {
        if let Ok(emulation) = serde_json::from_value::<WreqEmulation>(Value::String(candidate)) {
            return Ok(emulation);
        }
    }
    Err(format!("unknown emulation profile: '{input}'"))
}

/// Parses an OS name (`windows`, `macos`, `linux`, `android`, `ios`).
pub fn parse_emulation_os(input: &str) -> Result<EmulationOS, String> {
    for candidate in name_candidates(input) {
        if let Ok(os) = serde_json::from_value::<EmulationOS>(Value::String(candidate)) {
            return Ok(os);
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
    Plain(WreqEmulation),
    /// Profile plus OS / fingerprint toggles.
    Detailed(EmulationOption),
}

impl EmulationConfig {
    /// Builds from a plain profile name.
    pub fn from_name(name: &str) -> Result<Self, String> {
        Ok(Self::Plain(parse_emulation(name)?))
    }

    /// Builds from a profile name and optional OS / toggles.
    pub fn detailed(
        name: &str,
        os: Option<&str>,
        skip_http2: bool,
        skip_headers: bool,
    ) -> Result<Self, String> {
        let emulation = parse_emulation(name)?;
        let emulation_os = match os {
            Some(os) => parse_emulation_os(os)?,
            None => EmulationOS::default(),
        };
        // `EmulationOption`'s `TypedBuilder` is type-state based, so every
        // setter must be called in a single chain.
        let option = EmulationOption::builder()
            .emulation(emulation)
            .emulation_os(emulation_os)
            .skip_http2(skip_http2)
            .skip_headers(skip_headers)
            .build();
        Ok(Self::Detailed(option))
    }

    /// Applies the emulation onto a `wreq` client builder.
    pub fn apply(self, builder: wreq::ClientBuilder) -> wreq::ClientBuilder {
        match self {
            EmulationConfig::Plain(e) => builder.emulation(e),
            EmulationConfig::Detailed(o) => builder.emulation(o),
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
        WreqEmulation::VARIANTS.iter().map(profile_name).collect()
    }

    /// Picks a random profile, optionally restricted to a browser family
    /// (`chrome`, `firefox`, `safari`, `opera`, `edge`, `okhttp`, …). Throws
    /// when the family has no profiles, so a typo is loud instead of silently
    /// returning the same value as `random()`.
    pub fn random(family: Option<&str>) -> PhpResult<String> {
        let candidates = match family {
            Some(family) => variants_for_family(family),
            None => WreqEmulation::VARIANTS.iter().collect(),
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
            .map(|e| profile_name(e))
            .collect()
    }

    /// Whether the given profile name is recognized.
    pub fn exists(name: &str) -> bool {
        parse_emulation(name).is_ok()
    }
}
