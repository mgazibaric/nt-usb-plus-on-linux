//! Looks up the newest firmware RØDE has released for the mic. The app only
//! tells the user about it; installing firmware is left to RØDE Central.

use std::time::Duration;

use serde_json::Value;

/// The list RØDE Central reads its update information from
const MANIFEST_URL: &str = "https://update.rode.com/rode-devices-manifest.json";
const TIMEOUT: Duration = Duration::from_secs(8);

/// Version of the released NT-USB+ firmware, e.g. "1.0.9".
pub fn latest() -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    let manifest =
        agent.get(MANIFEST_URL).call().and_then(|mut r| r.body_mut().read_to_string()).map_err(|e| e.to_string())?;
    parse(&manifest).ok_or_else(|| "the update list has no NT-USB+ entry".to_string())
}

fn parse(manifest: &str) -> Option<String> {
    let manifest: Value = serde_json::from_str(manifest).ok()?;
    let version = manifest.get("ntusbplus-manifest")?.get("main-version")?.get("update-version")?.as_str()?;
    numbers(version).map(|_| version.to_string())
}

fn numbers(version: &str) -> Option<Vec<u32>> {
    version.split('.').map(|part| part.parse().ok()).collect()
}

/// Unreadable versions never count as newer.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    match (numbers(candidate), numbers(installed)) {
        (Some(candidate), Some(installed)) => candidate > installed,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_release_not_the_beta() {
        let manifest = r#"{ "name": "rode-devices-manifest", "ntusbplus-manifest": {
            "main-version": { "update-version": "1.0.9", "firmware-URL": "update.rode.com/ntusbplus/firmware.bin" },
            "beta-version": { "update-version": "1.1.0", "firmware-URL": "update.rode.com/ntusbplus/firmware.bin" } } }"#;
        assert_eq!(parse(manifest).as_deref(), Some("1.0.9"));
        assert_eq!(parse("{}"), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"ntusbplus-manifest":{"main-version":{"update-version":"soon"}}}"#), None);
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("1.1.0", "1.0.9"));
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(is_newer("2.0", "1.9.9"));
        assert!(!is_newer("1.0.9", "1.0.9"));
        assert!(!is_newer("1.0.8", "1.0.9"));
        assert!(!is_newer("1.1.0", ""));
        assert!(!is_newer("", "1.0.9"));
    }
}
