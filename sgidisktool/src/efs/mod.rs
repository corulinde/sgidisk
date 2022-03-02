use std::process::exit;
use clap::ArgMatches;

/// Volume Header tool entry point
pub(crate) fn subcommand(disk_file_name: &str, cli_matches: &ArgMatches) {
  let partition_id = cli_matches.
  let mut vol = crate::OpenVolume::open_or_quit(disk_file_name);


  match cli_matches.subcommand_name() {
    // EFS tool
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

pub(crate)