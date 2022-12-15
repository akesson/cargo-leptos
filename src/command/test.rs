use crate::config::Config;
use crate::ext::anyhow::{Context, Result};
use crate::logger::GRAY;
use crate::task::compile::{front_cargo_process, server_cargo_process};

pub async fn run(conf: &Config) -> Result<()> {
    let (line, mut proc) = server_cargo_process("test", conf).dot()?;

    proc.wait().await.dot()?;
    log::info!("Cargo server tests finished {}", GRAY.paint(line));

    let (line, mut proc) = front_cargo_process("test", false, conf).dot()?;

    proc.wait().await.dot()?;
    log::info!("Cargo front tests finished {}", GRAY.paint(line));
    Ok(())
}
