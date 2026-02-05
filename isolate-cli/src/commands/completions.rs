use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};

use crate::Cli;

#[derive(Parser, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: Shell,
}

pub fn completions_command(args: CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(args.shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(())
}
