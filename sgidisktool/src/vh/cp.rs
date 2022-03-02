use std::fs;
use std::path::PathBuf;
use std::process::exit;

use clap::ArgMatches;
use glob::Pattern;

use crate::OpenVolume;

/// Volume Header File copy entry point
pub(crate) fn subcommand(disk_file_name: &str, cli_matches: &ArgMatches) {
  let verbose = cli_matches.is_present("verbose");

  // Compile glob pattern from source argument
  let src = cli_matches.value_of("src").unwrap();
  let src_pattern = match Pattern::new(src) {
    Ok(p) => p,
    Err(e) => {
      eprintln!("Error compiling glob pattern from '{}': {:?}", src, e);
      exit(crate::exit_codes::GLOB_ERR);
    }
  };

  // Figure out whether dest argument is a directory
  let dest = cli_matches.value_of("dest").unwrap();
  let dest_is_dir = match fs::metadata(dest) {
    Ok(meta) => meta.is_dir(),
    Err(_) => false
  };

  // Open volume and find matching volume header files
  let mut vol = crate::OpenVolume::open_or_quit(disk_file_name);
  let matches = matches(&vol, &src_pattern);
  let num_matches = matches.len();

  // If there is more than one matching file, they need to go to a named directory
  if num_matches > 1 && !dest_is_dir {
    eprintln!("There were {} matching files but '{}' is not a directory!", num_matches, dest);
    exit(crate::exit_codes::CLI_ARG_ERROR);
  }

  // Copy files out
  for id in matches {
    cp(&mut vol, id, dest, dest_is_dir, verbose);
  }
}

// Copy indicated file to destination
fn cp(vol: &mut OpenVolume, id: usize, dest: &str, dest_is_dir: bool, verbose: bool) {
  let vol_file = &mut vol.disk_file;
  let vh_file = &vol.volume_header.files[id];
  let vh_file_name = vh_file.file_name.as_ref().unwrap();

  // If destination is directory then append VH file name, otherwise use dest verbatim
  let mut path = PathBuf::with_capacity(2);
  path.push(dest);
  if dest_is_dir {
    path.push(vh_file_name);
  }

  // Open destination file for writing
  let mut dest_file = match fs::File::create(&path) {
    Ok(f) => f,
    Err(e) => {
      eprintln!("Error opening {:?}: {:?}", &path, e);
      exit(crate::exit_codes::IO_ERR);
    }
  };

  // Perform copy
  let src_start = vh_file.block_start * sgidisklib::efs::EFS_BLOCK_SZ as u64;
  let src_len = vh_file.file_sz;
  match crate::cp(vol_file, src_start, src_len, &mut dest_file, 0) {
    Ok(_) => if verbose {
      println!("{} -> {}", vh_file_name, path.to_string_lossy());
    },
    Err(e) => {
      eprintln!("Error: {} -> {:?}: {:?}", vh_file_name, &path, &e);
    }
  }
}

/// Find matching Volume Header File IDs based on glob pattern
fn matches(vol: &OpenVolume, glob: &Pattern) -> Vec<usize> {
  let files = &vol.volume_header.files;
  files.iter().enumerate()
    .filter(|(_id, vf, )| vf.in_use())
    .filter(|(_id, vf, )| match vf.file_name.as_ref() {
      Some(name) => glob.matches_with(name.as_str(), crate::GLOB_OPT),
      None => false
    })
    .map(|(id, _vf)| id)
    .collect::<Vec<usize>>()
}