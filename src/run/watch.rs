use crate::ext::anyhow::{anyhow, Context, Result};
use crate::{
    logger::GRAY,
    path::{remove_nested, PathBufExt, PathExt},
    sync::{oneshot_when, shutdown_msg},
    util::{SenderAdditions, StrAdditions},
    Config, Msg, MSG_BUS,
};
use camino::Utf8PathBuf;
use itertools::Itertools;
use notify::{DebouncedEvent, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::{fmt::Display, time::Duration};
use tokio::task::JoinHandle;

pub async fn spawn(config: &Config) -> Result<JoinHandle<()>> {
    let mut paths = vec!["src".to_canoncial_dir()?];
    if let Some(style_file) = &config.leptos.style_file {
        paths.push(style_file.clone().without_last().to_canonicalized().dot()?);
    }

    let assets_dir = if let Some(dir) = &config.leptos.assets_dir {
        let assets_root = dir.to_canonicalized().dot()?;
        paths.push(assets_root.clone());
        Some(assets_root)
    } else {
        None
    };

    let paths = remove_nested(paths);

    log::info!("Watching folders {}", GRAY.paint(paths.iter().join(", ")));

    Ok(tokio::spawn(async move {
        run(&paths, vec![], assets_dir).await
    }))
}

async fn run(paths: &[Utf8PathBuf], exclude: Vec<Utf8PathBuf>, assets_dir: Option<Utf8PathBuf>) {
    let (sync_tx, sync_rx) = std::sync::mpsc::channel::<DebouncedEvent>();

    std::thread::spawn(move || {
        while let Ok(event) = sync_rx.recv() {
            match Watched::try_new(event) {
                Ok(Some(watched)) => handle(watched, &exclude, &assets_dir),
                _ => {}
            }
        }
        log::debug!("Watching stopped");
    });

    let mut watcher = notify::watcher(sync_tx, Duration::from_millis(200))
        .expect("failed to build file system watcher");

    for path in paths {
        if let Err(e) = watcher.watch(&path, RecursiveMode::Recursive) {
            log::error!("Watcher could not watch {path:?} due to {e:?}");
        }
    }

    if let Err(e) = oneshot_when(shutdown_msg, "Watch").await {
        log::trace!("Watcher stopped due to: {e:?}");
    }
}

fn handle(watched: Watched, exclude: &[Utf8PathBuf], assets_dir: &Option<Utf8PathBuf>) {
    if let Some(path) = watched.path() {
        if exclude.contains(path) {
            return;
        }
    }

    if let Some(assets_dir) = assets_dir {
        if watched.path_starts_with(assets_dir) {
            log::debug!("Watcher asset change {}", GRAY.paint(watched.to_string()));
            MSG_BUS.send_logged("Watcher", Msg::AssetsChanged(watched));
            return;
        }
    }

    match watched.path_ext() {
        Some("rs") => {
            log::debug!("Watcher source change {}", GRAY.paint(watched.to_string()));
            MSG_BUS.send_logged("Watcher", Msg::SrcChanged)
        }
        Some(ext) if ["scss", "sass", "css"].contains(&ext.to_lowercase().as_str()) => {
            log::debug!("Watcher style change {}", GRAY.paint(watched.to_string()));
            MSG_BUS.send_logged("Watcher", Msg::StyleChanged)
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    Remove(Utf8PathBuf),
    Rename(Utf8PathBuf, Utf8PathBuf),
    Write(Utf8PathBuf),
    Create(Utf8PathBuf),
    Rescan,
}

fn convert(p: PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(p).map_err(|e| anyhow!("Could not convert to a Utf8PathBuf: {e:?}"))
}
impl Watched {
    fn try_new(event: DebouncedEvent) -> Result<Option<Self>> {
        use DebouncedEvent::{
            Chmod, Create, Error, NoticeRemove, NoticeWrite, Remove, Rename, Rescan, Write,
        };

        Ok(match event {
            Chmod(_) | NoticeRemove(_) | NoticeWrite(_) => None,
            Create(f) => Some(Self::Create(convert(f)?)),
            Remove(f) => Some(Self::Remove(convert(f)?)),
            Rename(f, t) => Some(Self::Rename(convert(f)?, convert(t)?)),
            Write(f) => Some(Self::Write(convert(f)?)),
            Rescan => Some(Self::Rescan),
            Error(e, Some(p)) => {
                log::error!("Watcher error watching {p:?}: {e:?}");
                None
            }
            Error(e, None) => {
                log::error!("Watcher error: {e:?}");
                None
            }
        })
    }

    pub fn path_ext(&self) -> Option<&str> {
        self.path().map(|p| p.as_str())
    }

    pub fn path(&self) -> Option<&Utf8PathBuf> {
        match self {
            Self::Remove(p) | Self::Rename(p, _) | Self::Write(p) | Self::Create(p) => Some(&p),
            Self::Rescan => None,
        }
    }

    pub fn path_starts_with(&self, path: &Utf8PathBuf) -> bool {
        match self {
            Self::Write(p) | Self::Create(p) | Self::Remove(p) => p.starts_with(path),
            Self::Rename(fr, to) => fr.starts_with(path) || to.starts_with(path),
            Self::Rescan => false,
        }
    }
}

impl Display for Watched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create(p) => write!(f, "create {p:?}"),
            Self::Remove(p) => write!(f, "remove {p:?}"),
            Self::Write(p) => write!(f, "write {p:?}"),
            Self::Rename(fr, to) => write!(f, "rename {fr:?} -> {to:?}"),
            Self::Rescan => write!(f, "rescan"),
        }
    }
}
