use clap::Parser;
use std::path::PathBuf;

fn main() {
    let args = Cli::parse();
    pretty_env_logger::init();
    canva_note::app::run(args.file);
}

#[derive(clap::Parser)]
struct Cli {
    file: PathBuf,
}
