//! Shared newtypes and enums used across the CLI and the shared layer.
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Arch {
    #[serde(rename = "linux-64")]
    Linux64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetPlatform(Arch);

impl TargetPlatform {
    pub fn arch(&self) -> Arch {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeepstreamVersion {
    #[serde(rename = "7.1")]
    V7_1,
    #[serde(rename = "8.0")]
    V8_0,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerSize {
    #[serde(rename = "4cpu")]
    #[default]
    Cpu4,
    #[serde(rename = "8cpu")]
    Cpu8,
    #[serde(rename = "16cpu")]
    Cpu16,
    #[serde(rename = "32cpu")]
    Cpu32,
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Linux64 => "linux-64",
            Self::LinuxAarch64 => "linux-aarch64",
        })
    }
}

impl FromStr for Arch {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linux-64" => Ok(Self::Linux64),
            "linux-aarch64" => Ok(Self::LinuxAarch64),
            other => anyhow::bail!("unknown arch {other:?}; expected linux-64 or linux-aarch64"),
        }
    }
}

impl fmt::Display for DeepstreamVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::V7_1 => "7.1",
            Self::V8_0 => "8.0",
        })
    }
}

impl FromStr for DeepstreamVersion {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "7.1" => Ok(Self::V7_1),
            "8.0" => Ok(Self::V8_0),
            other => anyhow::bail!("unknown DeepStream version {other:?}; expected 7.1 or 8.0"),
        }
    }
}

#[cfg(test)]
mod enum_tests {
    use super::*;

    #[test]
    fn arch_parses_known() {
        assert_eq!("linux-64".parse::<Arch>().unwrap(), Arch::Linux64);
        assert_eq!("linux-aarch64".parse::<Arch>().unwrap(), Arch::LinuxAarch64);
    }

    #[test]
    fn arch_rejects_unknown() {
        assert!("osx-arm64".parse::<Arch>().is_err());
    }

    #[test]
    fn deepstream_version_round_trips_via_serde() {
        let yaml = "\"7.1\"\n";
        let v: DeepstreamVersion = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(v, DeepstreamVersion::V7_1);
        let out = serde_yaml::to_string(&DeepstreamVersion::V8_0).unwrap();
        assert_eq!(out.trim(), "'8.0'");
    }

    #[test]
    fn runner_size_default_is_4cpu() {
        assert_eq!(RunnerSize::default(), RunnerSize::Cpu4);
    }

    #[test]
    fn runner_size_serde() {
        let v: RunnerSize = serde_yaml::from_str("16cpu").unwrap();
        assert_eq!(v, RunnerSize::Cpu16);
        assert!(serde_yaml::from_str::<RunnerSize>("2cpu").is_err());
    }
}

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha40(String);

impl Sha40 {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());
        let s = s.into();
        if !re.is_match(&s) {
            anyhow::bail!("not a 40-char lowercase hex sha: {s:?}");
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha40 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha40 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecipeName(String);

impl RecipeName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecipeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GithubRepoUrl {
    pub owner: String,
    pub repo: String,
}

impl GithubRepoUrl {
    pub fn parse(url: &str) -> anyhow::Result<Self> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^https://github\.com/([^/]+)/([^/]+?)(?:\.git)?/?$").unwrap()
        });
        let caps = re
            .captures(url)
            .ok_or_else(|| anyhow::anyhow!("not a GitHub https URL: {url:?}"))?;
        Ok(Self {
            owner: caps[1].to_string(),
            repo: caps[2].to_string(),
        })
    }
}

impl<'de> Deserialize<'de> for GithubRepoUrl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GithubRepoUrl {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("https://github.com/{}/{}", self.owner, self.repo))
    }
}

/// Exactly one of `rev` or `ref` is set in `pixi_native_packages.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GitVersion {
    Rev(Sha40),
    Ref(String),
}

impl GitVersion {
    pub fn from_optional(rev: Option<Sha40>, git_ref: Option<String>) -> anyhow::Result<Self> {
        match (rev, git_ref) {
            (Some(r), None) => Ok(Self::Rev(r)),
            (None, Some(r)) => Ok(Self::Ref(r)),
            (Some(_), Some(_)) => {
                anyhow::bail!("entry has both `rev` and `ref`; exactly one is required")
            }
            (None, None) => {
                anyhow::bail!("entry has neither `rev` nor `ref`; exactly one is required")
            }
        }
    }
}

#[cfg(test)]
mod newtype_tests {
    use super::*;

    #[test]
    fn sha40_accepts_valid() {
        let s = Sha40::new("4110a9a40736b555c7419119ef6c607951563745").unwrap();
        assert_eq!(s.as_str(), "4110a9a40736b555c7419119ef6c607951563745");
    }

    #[test]
    fn sha40_rejects_uppercase() {
        assert!(Sha40::new("4110A9A40736b555c7419119ef6c607951563745").is_err());
    }

    #[test]
    fn sha40_rejects_short() {
        assert!(Sha40::new("4110a9a40736").is_err());
    }

    #[test]
    fn sha40_rejects_non_hex() {
        assert!(Sha40::new("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn github_url_parses_https() {
        let u = GithubRepoUrl::parse("https://github.com/Greenroom-Robotics/mise").unwrap();
        assert_eq!(u.owner, "Greenroom-Robotics");
        assert_eq!(u.repo, "mise");
    }

    #[test]
    fn github_url_strips_dot_git() {
        let u = GithubRepoUrl::parse("https://github.com/foo/bar.git").unwrap();
        assert_eq!(u.repo, "bar");
    }

    #[test]
    fn github_url_rejects_non_github() {
        assert!(GithubRepoUrl::parse("https://gitlab.com/foo/bar").is_err());
    }

    #[test]
    fn github_url_rejects_http() {
        assert!(GithubRepoUrl::parse("http://github.com/foo/bar").is_err());
    }

    #[test]
    fn gitversion_requires_exactly_one() {
        let sha = Sha40::new("4110a9a40736b555c7419119ef6c607951563745").unwrap();
        assert!(GitVersion::from_optional(Some(sha.clone()), None).is_ok());
        assert!(GitVersion::from_optional(None, Some("main".into())).is_ok());
        assert!(GitVersion::from_optional(Some(sha), Some("main".into())).is_err());
        assert!(GitVersion::from_optional(None, None).is_err());
    }
}

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PixiNativeEntry {
    pub name: String,
    pub url: GithubRepoUrl,
    pub version: GitVersion,
    pub subdir: Option<PathBuf>,
    pub runner_size: RunnerSize,
}

#[derive(Deserialize)]
struct PixiNativeEntryRaw {
    name: String,
    url: GithubRepoUrl,
    #[serde(default)]
    rev: Option<Sha40>,
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    subdir: Option<PathBuf>,
    #[serde(default, rename = "runner-size")]
    runner_size: Option<RunnerSize>,
}

impl TryFrom<PixiNativeEntryRaw> for PixiNativeEntry {
    type Error = anyhow::Error;
    fn try_from(raw: PixiNativeEntryRaw) -> Result<Self, Self::Error> {
        let version = GitVersion::from_optional(raw.rev, raw.git_ref)
            .map_err(|e| anyhow::anyhow!("entry {:?}: {e}", raw.name))?;
        Ok(Self {
            name: raw.name,
            url: raw.url,
            version,
            subdir: raw.subdir,
            runner_size: raw.runner_size.unwrap_or_default(),
        })
    }
}

#[derive(Deserialize)]
struct PixiNativeManifestRaw {
    packages: Vec<PixiNativeEntryRaw>,
}

#[derive(Debug, Clone)]
pub struct PixiNativeManifest {
    pub packages: Vec<PixiNativeEntry>,
}

impl PixiNativeManifest {
    pub fn from_yaml_str(yaml: &str) -> anyhow::Result<Self> {
        let raw: PixiNativeManifestRaw = serde_yaml::from_str(yaml)?;
        let packages = raw
            .packages
            .into_iter()
            .map(PixiNativeEntry::try_from)
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self { packages })
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/pixi_native_packages.yaml");

    #[test]
    fn rejects_entry_with_both_rev_and_ref() {
        let err = PixiNativeManifest::from_yaml_str(FIXTURE).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("pkg_with_both"), "got: {msg}");
        assert!(msg.contains("both"), "got: {msg}");
    }

    #[test]
    fn parses_valid_subset() {
        let yaml = r#"
packages:
  - name: pkg_with_rev
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
  - name: pkg_with_ref
    url: https://github.com/example/repo.git
    ref: main
    subdir: packages/inner
    runner-size: 8cpu
"#;
        let m = PixiNativeManifest::from_yaml_str(yaml).unwrap();
        assert_eq!(m.packages.len(), 2);
        assert_eq!(m.packages[0].name, "pkg_with_rev");
        assert!(matches!(m.packages[0].version, GitVersion::Rev(_)));
        assert_eq!(m.packages[0].runner_size, RunnerSize::Cpu4);
        assert_eq!(
            m.packages[1].subdir.as_deref(),
            Some(std::path::Path::new("packages/inner"))
        );
        assert_eq!(m.packages[1].runner_size, RunnerSize::Cpu8);
    }
}

impl FromStr for TargetPlatform {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl FromStr for RecipeName {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl Default for TargetPlatform {
    fn default() -> Self {
        Self(Arch::Linux64)
    }
}
