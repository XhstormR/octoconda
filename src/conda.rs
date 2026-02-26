// SPDX-License-Identifier: GPL-3.0-or-later
// © Tobias Hunger <tobias.hunger@gmail.com>

use rattler_conda_types::{
    Channel, ChannelConfig, MatchSpec, ParseStrictness, Platform, RepoDataRecord,
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

    let spec = MatchSpec::from_str("*", ParseStrictness::Lenient)?;

    let repo_data = Gateway::new()
        .query(std::iter::once(channel), platforms, std::iter::once(spec))
        .await?;

    let mut result = Vec::new();
    for rd in repo_data {
        for rdi in rd.iter() {
            result.push(rdi.clone())
        }
    }
    Ok(result)
}
