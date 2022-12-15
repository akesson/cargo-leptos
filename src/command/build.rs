use crate::{
    config::Config,
    ext::{
        anyhow::{Context, Result},
        fs,
    },
    task::{compile, ChangeSet},
};

pub async fn run(conf: &Config) -> Result<()> {
    if conf.site_root().exists() {
        fs::rm_dir_content(conf.site_root()).await.dot()?;
    }
    let changes = ChangeSet::all_changes();

    compile::server(conf, &changes).await.await??;
    compile::front(conf, &changes).await.await??;
    compile::assets(conf, &changes, true).await.await??;
    compile::style(conf, &changes).await.await??;
    Ok(())
}
