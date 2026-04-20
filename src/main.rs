//! pconv binary entry point. Keep this tiny — unwrap/expect lives here
//! so library code can stay anyhow-only per the workspace convention.

use clap::Parser;
use portaconv::cli::{run, Cli};

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("pconv: {err:#}");
        std::process::exit(1);
    }
}
