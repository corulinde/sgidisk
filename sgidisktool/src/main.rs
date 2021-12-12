use std::fs;
use std::process::exit;

use clap::{load_yaml, App};
use tabled::Style;

mod exit_codes;
mod hash;
mod vh;

/// Main sgidisktool CLI entry point
fn main() {
  // Parse CLI arguments
  let cli_yaml = load_yaml!("cli.yaml");
  let cli_matches = App::from_yaml(cli_yaml).get_matches();

  // Open disk image
  let disk_file_name = cli_matches.value_of("file").unwrap();
  match cli_matches.subcommand_name() {
    // Volume Header tool
    Some("vh") => vh::subcommand(disk_file_name, cli_matches.subcommand_matches("vh").unwrap()),
    // Hash tool
    Some("hash") => hash::subcommand(disk_file_name, cli_matches.subcommand_matches("hash").unwrap()),

    // Unimplemented / unknown sub-command
    Some(subcommand_name) => {
      eprintln!("Unimplemented sub-command: {}", subcommand_name);
      exit(exit_codes::CLI_ARG_ERROR);
    }

    // Something strange happened?
    _ => {
      eprintln!("Unimplemented CLI combination: {:?}", &cli_matches);
      exit(exit_codes::CLI_ARG_ERROR);
    }
  }
}

/// Open disk image / Volume Header
#[derive(Debug)]
pub(crate) struct OpenVolume<'a> {
  pub(crate) disk_file_name: &'a str,
  pub(crate) disk_file_meta: fs::Metadata,
  pub(crate) disk_file: fs::File,
  pub(crate) volume_header: sgidisklib::volhdr::SgidiskVolume,
}

impl<'a> OpenVolume<'a> {
  /// Open a disk image and read the Volume Header
  pub(crate) fn open(disk_file_name: &'a str) -> Result<Self, String> {
    // Read metadata of file
    let disk_file_meta = match fs::metadata(disk_file_name) {
      Ok(disk_file_meta) => disk_file_meta,
      Err(e) => return Err(format!("Unable to get file metadata for disk image '{}': {:?}", disk_file_name, &e))
    };

    // Open file
    let mut disk_file = match fs::File::open(disk_file_name) {
      Ok(disk_file) => disk_file,
      Err(e) => return Err(format!("Unable to open disk image '{}': {:?}", disk_file_name, &e))
    };

    // Read volume header
    let volume_header = match sgidisklib::volhdr::SgidiskVolume::read(&mut disk_file) {
      Ok(volume_header) => volume_header,
      Err(e) => return Err(format!("Unable to read Volume Header from disk image '{}': {:?}", disk_file_name, &e))
    };

    Ok(Self {
      disk_file_name,
      disk_file_meta,
      disk_file,
      volume_header,
    })
  }

  /// Open a disk image and read the Volume Header, or quit if there is an error
  pub(crate) fn open_or_quit(disk_file_name: &'a str) -> Self {
    let vol = match Self::open(disk_file_name) {
      Ok(vol) => vol,
      Err(e) => {
        eprintln!("Error: {}", &e);
        exit(crate::exit_codes::VH_OPEN_ERR);
      }
    };

    vol
  }
}

/// Standard table formatting
pub(crate) fn table_fmt() -> Style {
  Style::pseudo_clean()
}