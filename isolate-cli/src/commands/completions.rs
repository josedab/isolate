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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completions_args_bash() {
        use clap::Parser;
        let args = CompletionsArgs::try_parse_from(["completions", "bash"]).unwrap();
        assert_eq!(args.shell, Shell::Bash);
    }

    #[test]
    fn test_completions_args_zsh() {
        use clap::Parser;
        let args = CompletionsArgs::try_parse_from(["completions", "zsh"]).unwrap();
        assert_eq!(args.shell, Shell::Zsh);
    }

    #[test]
    fn test_completions_args_fish() {
        use clap::Parser;
        let args = CompletionsArgs::try_parse_from(["completions", "fish"]).unwrap();
        assert_eq!(args.shell, Shell::Fish);
    }

    #[test]
    fn test_completions_args_invalid() {
        use clap::Parser;
        assert!(CompletionsArgs::try_parse_from(["completions", "invalid"]).is_err());
    }
}
