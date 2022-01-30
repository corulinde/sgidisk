use std::process::exit;
use clap::ArgMatches;

mod info;
mod cp;

/// Volume Header tool entry point
pub(crate) fn subcommand(disk_file_name: &str, cli_matches: &ArgMatches) {
  match cli_matches.subcommand_name() {
    // Volume Header tool
    Some("info") => info::subcommand(disk_file_name, cli_matches.subcommand_matches("info").unwrap()),
    Some("cp") => cp::subcommand(disk_file_name, cli_matches.subcommand_matches("cp").unwrap()),

    // Unimplemented / unknown sub-command
    Some(subcommand_name) => {
      eprintln!("Unimplemented sub-command: {}", subcommand_name);
      exit(super::exit_codes::CLI_ARG_ERROR);
    }

    // Something strange happened?
    _ => {
      eprintln!("Unimplemented CLI combination: {:?}", &cli_matches);
      exit(super::exit_codes::CLI_ARG_ERROR);
    }
  }
}