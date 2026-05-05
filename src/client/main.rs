use clap::Parser;
use fuser::{Config, MountOption, SessionACL, mount2};
use libazpfs::client::FUSEFilesytem;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser)]
struct Args {
    /// Mount path
    #[arg(required = true)]
    mountpoint: PathBuf,
}

fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(
                        if cfg!(debug_assertions) {
                            "azpfsd=debug,libazpfs=debug,warn"
                        } else {
                            "warn"
                        },
                    )
                }),
        )
        .pretty()
        .init();

    let args = Args::parse();
    let fs = FUSEFilesytem::new();

    info!(mountpoint = args.mountpoint.to_str(), "azpfsd starting");

    let mut config = Config::default();
    config.mount_options.push(MountOption::AutoUnmount);
    config.acl = SessionACL::All;
    mount2(fs, args.mountpoint, &config)
}
