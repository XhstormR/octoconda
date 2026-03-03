// SPDX-License-Identifier: GPL-3.0-or-later
// © Tobias Hunger <tobias.hunger@gmail.com>

use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    path::Path,
};

use anyhow::Context;
use rattler_conda_types::Platform;
use serde::Deserialize;

use crate::types::Repository;

/// Derive the conda package name from the config: use the explicit name if
/// provided, otherwise fall back to the repository name. The result is
/// lowercased because conda package names are case-insensitive.
pub fn conda_package_name(name: Option<&str>, repo: &str) -> String {
    name.unwrap_or(repo).to_lowercase()
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    String(String),
    List(Vec<String>),
}

#[derive(Deserialize)]
pub struct TomlPackage {
    pub name: Option<String>,
    #[serde(rename = "release-prefix")]
    pub release_prefix: Option<String>,
    pub repository: String,
    pub platforms: Option<HashMap<Platform, StringOrList>>,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub repository: Repository,
    pub platforms: HashMap<Platform, Vec<regex::Regex>>,
}

const ARCHIVE: &str =
    "\\.tar\\.gz|\\.tar\\.xz|\\.tar\\.bz2|\\.tar\\.zstd?|\\.tgz|\\.txz|\\.tbz|\\.zip";
const COMPRESSED: &str = "\\.gz|\\.xz|\\.zstd?|\\.bz2";
const VERSION: &str = "v\\d+([\\.[^\\.]+])*";
const VER: &str = "([\\._-]v\\d+([\\.[^\\.]+])*)?";

const X86: &str = "(intel[_-]?32|i?[3-6]86|x86|32[_-]?bit)";
const X64: &str = "(intel[_-]?64|x86[_-]64|amd[_-]?64|x64|64[_-]?bit)";
const ARM: &str = "(arm[_-]?64|aarch[_-]?64)";

const APPLE: &str = "(apple|darwin|mac([\\._-]?os([\\._-]?x)?)?|os[\\._-]?x)";
const WINDOWS: &str = "(windows|win(32|64)?)";

fn default_platforms() -> HashMap<Platform, Vec<String>> {
    fn linux_patterns(arch: &str, width: usize) -> Vec<String> {
        let linux = format!("linux({width})?");
        let extra = format!("([._-]({VERSION}|x11|unknown|gh))*");
        let gnu = "gnu|glibc\\d*";
        let mut result = vec![
            format!(
                "{arch}[\\._-](unknown[\\._-])?{linux}[\\._-]musl{extra}({COMPRESSED}|{ARCHIVE})?$"
            ),
            format!("{arch}[\\._-]musl[\\._-]{linux}{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{linux}[\\._-]{arch}[\\._-]musl{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!(
                "{arch}([\\._-](unknown|{gnu}))?[\\._-]{linux}([\\._-]({gnu}))?{extra}({COMPRESSED}|{ARCHIVE})?$"
            ),
            format!(
                "{linux}([\\._-](unknown|{gnu}))?[\\._-]{arch}([\\._-]({gnu}))?{extra}({COMPRESSED}|{ARCHIVE})?$"
            ),
        ];
        if arch == X64 {
            result.push(format!(
                "{linux}([\\._-](unknown|gnu))?{extra}({COMPRESSED}|{ARCHIVE})?$"
            ));
        }
        if arch == ARM {
            result.push(format!(
                "{linux}[\\._-]arm([\\._-](unknown|gnu))?{extra}({COMPRESSED}|{ARCHIVE})?$"
            ));
        }
        result
    }

    fn mac_patterns(arch: &str) -> Vec<String> {
        let extra = format!("([._-]({VERSION}|unknown|gh))*");
        vec![
            format!("{arch}([\\._-]apple)?[\\._-]{APPLE}-15{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}-15[\\._-]{arch}{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}-15[\\._-](universal|all){extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{arch}([\\._-]apple)?[\\._-]{APPLE}-14{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}-14[\\._-]{arch}{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}-14[\\._-](universal|all){extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{arch}([\\._-]apple)?[\\._-]{APPLE}-13{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}-13[\\._-]{arch}{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}-13[\\._-](universal|all){extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{arch}([\\._-]apple)?[\\._-]{APPLE}{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}[\\._-]{arch}{extra}({COMPRESSED}|{ARCHIVE})?$"),
            format!("{APPLE}[\\._-](universal|all){extra}({COMPRESSED}|{ARCHIVE})?$"),
        ]
    }

    fn win_patterns(arch: &str, width: usize) -> Vec<String> {
        let mut result = vec![
            format!(
                "{arch}([\\._-]pc)?[\\._-]{WINDOWS}([\\._-]msvc)?{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"
            ),
            format!(
                "{arch}([\\._-]pc)?[\\._-]{WINDOWS}([\\._-]msvc)?{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"
            ),
            format!("{WINDOWS}([\\._-]msvc)?[\\._-]{arch}{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"),
            format!(
                "{arch}([\\._-]pc)?[\\._-]{WINDOWS}[\\._-]gnu(llvm)?{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"
            ),
            format!("{arch}.exe"),
        ];
        if arch != ARM {
            result.push(format!("win{width}{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"));
        } else {
            result.push(format!(
                "arm([\\._-]pc)?[\\._-]{WINDOWS}([\\._-]gnu(llvm)?)?{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"
            ));
            result.push(format!(
                "{WINDOWS}([\\._-]msvc)?[\\._-]arm{VER}([\\._-]exe)?({ARCHIVE}|\\.exe)?$"
            ));
        }
        result
    }

    HashMap::from([
        (Platform::Linux32, linux_patterns(X86, 32)),
        (Platform::Linux64, linux_patterns(X64, 64)),
        (Platform::LinuxAarch64, linux_patterns(ARM, 64)),
        (Platform::Osx64, mac_patterns(X64)),
        (Platform::OsxArm64, mac_patterns(ARM)),
        (Platform::Win32, win_patterns(X86, 32)),
        (Platform::Win64, win_patterns(X64, 64)),
        (Platform::WinArm64, win_patterns(ARM, 64)),
    ])
}

impl TryFrom<TomlPackage> for Package {
    type Error = anyhow::Error;

    fn try_from(value: TomlPackage) -> Result<Self, Self::Error> {
        let repository = Repository::try_from(value.repository.as_str())?;
        let name = conda_package_name(value.name.as_deref(), &repository.repo);

        let release_prefix = value.release_prefix.clone();

        let platforms = {
            let mut result = default_platforms();
            for (k, v) in value.platforms.unwrap_or_default().drain() {
                let strings = match v {
                    StringOrList::String(s) => {
                        if s == "null" {
                            result.remove(&k);
                            continue;
                        }

                        vec![s]
                    }
                    StringOrList::List(items) => items,
                };
                result.insert(k, strings);
            }

            result
                .drain()
                .map(|(k, v)| {
                    let re = v
                        .iter()
                        .map(|r| {
                            let pattern = if let Some(rp) = &release_prefix {
                                format!("^{rp}([\\._-].+)?[\\._-]{r}")
                            } else {
                                format!("(^|[\\._-]){r}")
                            };
                            regex::RegexBuilder::new(&pattern)
                                .case_insensitive(true)
                                .build()
                                .context(format!("failed to parse regex for platform {k}"))
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    Ok((k, re))
                })
                .collect::<anyhow::Result<HashMap<_, _>>>()?
        };

        Ok(Package {
            name,
            repository,
            platforms,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Conda {
    pub channel: String,
}

impl Conda {
    pub fn short_channel(&self) -> anyhow::Result<String> {
        if let Ok(channel_url) = url::Url::parse(&self.channel) {
            if channel_url.host_str() != Some("prefix.dev") {
                return Err(anyhow::anyhow!(
                    "Not a prefix channel, can not generate a channel name from this URL"
                ));
            }
            Ok(channel_url.path().to_string())
        } else {
            Ok(self.channel.clone())
        }
    }

    pub fn full_channel(&self) -> anyhow::Result<String> {
        let short_channel = self.short_channel()?;
        Ok(format!("https://prefix.dev/{short_channel}"))
    }
}

#[derive(serde::Deserialize)]
pub struct TomlConfig {
    pub packages: Vec<TomlPackage>,
    pub conda: Conda,
}

impl TryFrom<TomlConfig> for Config {
    type Error = anyhow::Error;

    fn try_from(mut value: TomlConfig) -> Result<Self, Self::Error> {
        // Check for duplicate package names across ALL entries (including deprecated).
        {
            let mut seen: HashMap<String, (&str, bool)> = HashMap::new();
            for tp in &value.packages {
                let repo = Repository::try_from(tp.repository.as_str())?;
                let name = conda_package_name(tp.name.as_deref(), &repo.repo);
                if let Some((prev_repo, prev_deprecated)) = seen.get(&name) {
                    if tp.deprecated || *prev_deprecated {
                        eprintln!(
                            "Note: Duplicate package name \"{name}\": \
                             produced by both \"{prev_repo}\" and \"{}\"\
                             (at least one is deprecated)",
                            tp.repository,
                        );
                    } else {
                        anyhow::bail!(
                            "Duplicate package name \"{name}\": \
                             produced by both \"{prev_repo}\" and \"{}\"",
                            tp.repository,
                        );
                    }
                }
                seen.insert(name, (&tp.repository, tp.deprecated));
            }
        }

        let packages: Vec<Package> = value
            .packages
            .drain(..)
            .filter(|tp| !tp.deprecated)
            .map(|tp| tp.try_into())
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(Config {
            packages,
            conda: value.conda,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub packages: Vec<Package>,
    pub conda: Conda,
}

impl Config {
    pub fn all_platforms(&self) -> HashSet<Platform> {
        self.packages
            .iter()
            .flat_map(|p| p.platforms.keys())
            .copied()
            .collect()
    }
}

pub fn parse_config(path: &Path) -> anyhow::Result<Config> {
    let contents = std::fs::read_to_string(path).context(format!(
        "Failed to read configuration file {}",
        path.display()
    ))?;
    let config: TomlConfig = toml::from_str(&contents).context(format!(
        "Failed to parse configuration file {}",
        path.display()
    ))?;

    config.try_into()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn get_patterns_for(release_prefix: &str) -> HashMap<Platform, Vec<regex::Regex>> {
        let rp = if release_prefix.is_empty() {
            None
        } else {
            Some(release_prefix.to_string())
        };
        let toml = TomlPackage {
            name: None,
            release_prefix: rp,
            repository: "foo/bar".to_string(),
            platforms: None,
            deprecated: false,
        };
        let package: super::Package = toml.try_into().unwrap();
        package.platforms
    }

    fn toml_config(packages: Vec<TomlPackage>) -> TomlConfig {
        TomlConfig {
            packages,
            conda: Conda {
                channel: "test-channel".to_string(),
            },
        }
    }

    #[test]
    fn test_duplicate_package_names_rejected() {
        let config = toml_config(vec![
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "alice/foo".to_string(),
                platforms: None,
                deprecated: false,
            },
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "bob/foo".to_string(),
                platforms: None,
                deprecated: false,
            },
        ]);
        let err = Config::try_from(config).unwrap_err();
        assert!(
            err.to_string().contains("Duplicate package name"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_duplicate_package_names_case_insensitive() {
        let config = toml_config(vec![
            TomlPackage {
                name: Some("Foo".to_string()),
                release_prefix: None,
                repository: "alice/something".to_string(),
                platforms: None,
                deprecated: false,
            },
            TomlPackage {
                name: Some("foo".to_string()),
                release_prefix: None,
                repository: "bob/other".to_string(),
                platforms: None,
                deprecated: false,
            },
        ]);
        let err = Config::try_from(config).unwrap_err();
        assert!(
            err.to_string().contains("Duplicate package name"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_explicit_name_conflicts_with_repo_name() {
        let config = toml_config(vec![
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "alice/foo".to_string(),
                platforms: None,
                deprecated: false,
            },
            TomlPackage {
                name: Some("foo".to_string()),
                release_prefix: None,
                repository: "bob/bar".to_string(),
                platforms: None,
                deprecated: false,
            },
        ]);
        let err = Config::try_from(config).unwrap_err();
        assert!(
            err.to_string().contains("Duplicate package name"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_unique_package_names_accepted() {
        let config = toml_config(vec![
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "alice/foo".to_string(),
                platforms: None,
                deprecated: false,
            },
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "bob/bar".to_string(),
                platforms: None,
                deprecated: false,
            },
        ]);
        Config::try_from(config).unwrap();
    }

    #[test]
    fn test_duplicate_with_deprecated_is_not_an_error() {
        let config = toml_config(vec![
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "alice/foo".to_string(),
                platforms: None,
                deprecated: true,
            },
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "bob/foo".to_string(),
                platforms: None,
                deprecated: false,
            },
        ]);
        let cfg = Config::try_from(config).unwrap();
        assert_eq!(cfg.packages.len(), 1);
        assert_eq!(cfg.packages[0].repository.owner, "bob");
    }

    #[test]
    fn test_duplicate_both_deprecated_is_not_an_error() {
        let config = toml_config(vec![
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "alice/foo".to_string(),
                platforms: None,
                deprecated: true,
            },
            TomlPackage {
                name: None,
                release_prefix: None,
                repository: "bob/foo".to_string(),
                platforms: None,
                deprecated: true,
            },
        ]);
        let cfg = Config::try_from(config).unwrap();
        assert!(cfg.packages.is_empty());
    }
}
