use anyhow::Context as _;
use clap::Parser;
use kube::config::{KubeConfigOptions, Kubeconfig};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "orchbus-operator", about = "Reconcile orchbus.io Tasks into agent pods")]
struct Args {
    /// Kubeconfig of the orchbus cluster; ~/.kube/config is never read.
    #[arg(long)]
    kubeconfig: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args = Args::parse();
    let kc = Kubeconfig::read_from(&args.kubeconfig).with_context(|| format!("reading {}", args.kubeconfig.display()))?;
    let config = kube::Config::from_custom_kubeconfig(kc, &KubeConfigOptions::default()).await?;
    let client = kube::Client::try_from(config)?;
    orchbus_operator::run(client, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await;
    Ok(())
}
