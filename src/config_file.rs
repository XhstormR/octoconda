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

fn default_platforms() -> HashMap<Platform, Vec<String>> {
    HashMap::from([
        (
            Platform::Linux32,
            vec![
                "(^|[\\._-])i686[\\._-](unknown[\\._-])?linux[\\._-]musl(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])i686[\\._-](unknown[\\._-])?linux([\\._-]gnu)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])linux[\\._-](i686|x86)([\\._-]unknown)?([\\._-]gnu|[\\._-]musl)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])linux32([\\._-]unknown)?([\\._-]gnu|[\\._-]musl)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
            ],
        ),
        (
            Platform::Linux64,
            vec![
                "(^|[\\._-])(x86_64|amd64|x64)[\\._-](unknown[\\._-])?linux[\\._-]musl(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])(x86_64|amd64|x64)[\\._-](unknown[\\._-])?linux([\\._-]gnu)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])linux[\\._-](x86_64|amd64|x64)([\\._-]unknown)?([\\._-]gnu|[\\._-]musl)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])linux64([\\._-]unknown)?([\\._-]gnu|[\\._-]musl)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
            ],
        ),
        (
            Platform::LinuxAarch64,
            vec![
                "(^|[\\._-])(arm64|aarch64)[\\._-](unknown[\\._-])?linux[\\._-]musl(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])(arm64|aarch64)[\\._-](unknown[\\._-])?linux([\\._-]gnu)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])linux[\\._-](arm64|aarch64)([\\._-]unknown)?([\\._-]gnu|[\\._-]musl)?(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
            ],
        ),
        (
            Platform::Osx64,
            vec![
                "(^|[\\._-])(amd64|x86_64|x64)[\\._-](apple[\\._-])?(darwin|macos|osx)(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])(darwin|macos|osx)[\\._-](amd64|x86_64|x64)(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])(darwin|macos|osx)(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
            ],
        ),
        (
            Platform::OsxArm64,
            vec![
                "(^|[\\._-])(arm64|aarch64)[\\._-](apple[\\._-])?(darwin|macos|osx)(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
                "(^|[\\._-])(darwin|macos|osx)[\\._-](arm64|aarch64)(\\.gz|\\.xz|\\.zst|\\.tar\\.gz|\\.tar\\.xz|\\.tgz|\\.txz|\\.zip)?$"
                    .to_string(),
            ],
        ),
        (
            Platform::Win32,
            vec![
                "(^|[\\._-])(x86|i686)[\\._-](pc)?[\\._-]windows([\\._-]msvc)?(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
                "(^|[\\._-])(windows|win(32|64)?)[\\._-](32-bit|i686|x86)(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
                "(^|[\\._-])win32(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
            ],
        ),
        (
            Platform::Win64,
            vec![
                "(^|[\\._-])(amd_64|x86_64|x64)([\\._-]pc)?[\\._-]windows([\\._-]msvc)?(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
                "(^|[\\._-])(windows|win(32|64)?)[\\._-](64-bit|amd64|x86_64|x64)(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
                "(^|[\\._-])win64(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
            ],
        ),
        (
            Platform::WinArm64,
            vec![
                "(^|[\\._-])(arm64|aarch64)([\\._-]pc)?[\\._-]windows([\\._-]msvc)?(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
                "(^|[\\._-])(windows|win(32|64)?)[\\._-](arm64|aarch64)(\\.gz|\\.xz|\\.zst|\\.zip|\\.exe)?$".to_string(),
            ],
        ),
    ])
}

impl TryFrom<TomlPackage> for Package {
    type Error = anyhow::Error;

    fn try_from(value: TomlPackage) -> Result<Self, Self::Error> {
        let repository = Repository::try_from(value.repository.as_str())?;
        let name = conda_package_name(value.name.as_deref(), &repository.repo);

        let release_prefix = value
            .release_prefix
            .or(value.name)
            .map(|s| s.to_lowercase());

        let platforms = {
            let mut result = default_platforms();
            for (k, v) in value.platforms.unwrap_or_default().drain() {
                let strings = match v {
                    StringOrList::String(s) => {
                        if s == "null" {
                            result.remove(&k);
                            continue;
                        }

                        if let Some(rp) = release_prefix.as_ref() {
                            let Some(current) = result.get(&k) else {
                                return Err(anyhow::anyhow!(format!(
                                    "Can not prepend to default platform key {k}"
                                )));
                            };
                            result.insert(
                                k,
                                current
                                    .iter()
                                    .map(|c| {
                                        let mut r = rp.to_string();
                                        r.push_str(&format!(".*{c}"));
                                        r
                                    })
                                    .collect::<Vec<_>>(),
                            );
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
                                format!("^{rp}.*{r}")
                            } else {
                                r.to_string()
                            };
                            regex::Regex::new(&pattern)
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
