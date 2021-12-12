use clap::ArgMatches;
use tabled::{Tabled, Table};

use sgidisklib::volhdr::PartitionType;
use crate::OpenVolume;

/// Volume Header info entry point
pub(crate) fn subcommand(disk_file_name: &str, _cli_matches: &ArgMatches) {
  // let json = cli_matches.is_present("json");

  let vol = crate::OpenVolume::open_or_quit(disk_file_name);

  print_vh(&vol);
}

/// Formatted print of Volume Header information
fn print_vh(vol: &OpenVolume) {
  let vh = &vol.volume_header;

  println!("Sector size: {} bytes", vh.sector_sz);
  println!("Command Tag Queueing: {} (depth {})", vh.ctq_enabled, vh.ctq_depth);
  println!("Root partition ID: {}", vh.root_partition);
  println!("Swap partition ID: {}", vh.swap_partition);

  println!();
  println!("Partitions:");
  print_partitions(vol);
  if vh.partitions.len() > 10 && vh.partitions[10].partition_type == PartitionType::EntireVolume {
    let p = &vh.partitions[10];
    let vol_end = (p.block_start + p.block_sz) * sgidisklib::efs::EFS_BLOCK_SZ as u64;
    let file_sz = vol.disk_file_meta.len();

    let comparison = if vol_end > file_sz {
      format!("past end of disk image by {} bytes!", vol_end - file_sz)
    } else if vol_end < file_sz {
      format!("smaller than disk image by {} bytes", file_sz - vol_end)
    } else {
      format!("equal to the disk image size at {} bytes", vol_end)
    };
    println!("Entire Volume (partition 10) is {}", comparison);
  }

  println!();
  if let Some(boot_file) = &vh.boot_file {
    println!("Boot file: {}", boot_file);
  } else {
    println!("No boot file listed.");
  }
  println!("Volume Directory:");
  print_voldir(vol);
}

/// Print partition table nicely
fn print_partitions(vol: &OpenVolume) {
  let file_sz = vol.disk_file_meta.len();
  let vh = &vol.volume_header;

  #[derive(Tabled)]
  struct DisplayPartition {
    id: usize,
    partition_type: PartitionType,
    start_block: u64,
    end_block: u64,
    size_blocks: u64,
    over_length: String,
  }

  let part_tab = vh.partitions.iter()
    .enumerate()
    .filter(|(_, p, )| p.in_use())
    .map(|(id, p, )| {
      let end_block = p.block_start + p.block_sz;
      let end_byte = end_block * sgidisklib::efs::EFS_BLOCK_SZ as u64;
      let over_length = if end_byte > file_sz {
        format!("Yes ({} bytes)", end_byte - file_sz)
      } else { "No".to_string() };

      DisplayPartition {
        id,
        partition_type: p.partition_type,
        start_block: p.block_start,
        end_block,
        size_blocks: p.block_sz,
        over_length,
      }
    })
    .collect::<Vec<DisplayPartition>>();

  print!("{}", Table::new(part_tab).with(crate::table_fmt()));
}

/// List table of files in volume directory
fn print_voldir(vol: &OpenVolume) {
  let file_sz = vol.disk_file_meta.len();
  let vh = &vol.volume_header;

  #[derive(Tabled)]
  struct DisplayFile {
    id: usize,
    file_name: String,
    start_block: u64,
    size_bytes: u64,
    over_length: String,
  }

  let file_tab = vh.files.iter()
    .enumerate()
    .filter(|(_, f, )| f.in_use())
    .map(|(id, f, )| {
      let end_bytes = f.block_start * sgidisklib::efs::EFS_BLOCK_SZ as u64;
      let over_length = if end_bytes > file_sz {
        format!("Yes ({} bytes)", end_bytes - file_sz)
      } else {
        "No".to_string()
      };
      DisplayFile {
        id,
        file_name: f.file_name.as_ref().unwrap().clone(),
        start_block: f.block_start,
        size_bytes: f.file_sz,
        over_length,
      }
    })
    .collect::<Vec<DisplayFile>>();

  print!("{}", Table::new(file_tab).with(crate::table_fmt()));
}