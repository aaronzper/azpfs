use std::path::PathBuf;

use clap::Parser;
use fuser::{Config, MountOption, SessionACL, mount2};
use libazpfs::client::FUSEFilesytem;

#[derive(Parser)]
struct Args {
    /// Mount path
    #[arg(required = true)]
    mountpoint: PathBuf,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let fs = FUSEFilesytem::new();

    println!("azpfsd starting at mountpoint {:?}", args.mountpoint);

    let mut config = Config::default();
    config.mount_options.push(MountOption::AutoUnmount);
    config.acl = SessionACL::All;
    mount2(fs, args.mountpoint, &config)
}
