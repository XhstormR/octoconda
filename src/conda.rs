// SPDX-License-Identifier: GPL-3.0-or-later
// © Tobias Hunger <tobias.hunger@gmail.com>

use rattler_conda_types::{
    Channel, ChannelConfig, MatchSpec, PackageNameMatcher, Platform, RepoDataRecord,
};
use rattler_repodata_gateway::Gateway;

use std::path::PathBuf;

pub async fn get_all_conda_packages(
    channel: &str,
    platforms: impl Iterator<Item = Platform> + Clone,
) -> Result<Vec<RepoDataRecord>, anyhow::Error> {
    let channel = Channel::from_str(
        channel,
        &ChannelConfig::default_with_root_dir(PathBuf::from(".")),
    )?;

    let spec = MatchSpec {
        name: Some(PackageNameMatcher::from(glob::Pattern::new("*").unwrap())),
        ..Default::default()
    };

    let gateway = Gateway::builder()
        .with_channel_config(rattler_repodata_gateway::ChannelConfig {
            default: rattler_repodata_gateway::SourceConfig {
                sharded_enabled: false,
                ..Default::default()
            },
            ..Default::default()
        })
        .finish();

    let repo_data = gateway
        .query(std::iter::once(channel), platforms, std::iter::once(spec))
        .await?;

    let mut result = Vec::new();
    for rd in repo_data {
        result.extend(rd.iter().cloned());
    }

    result.sort_by_key(|rdr| rdr.package_record.name.as_normalized().to_string());

    Ok(result)
}
