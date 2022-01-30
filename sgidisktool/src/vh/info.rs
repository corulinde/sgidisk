use std::collections::BTreeMap;
use clap::ArgMatches;
use tabled::{Tabled, Table};
use serde::Serialize;
use serde_json;

use sgidisklib::volhdr::{Partition, PartitionType, VolumeFile};
use crate::OpenVolume;

/// Volume Header info entry point
pub(crate) fn subcommand(disk_file_name: &str, cli_matches: &ArgMatches) {
  let json = cli_matches.is_present("json");

  let vol = crate::OpenVolume::open_or_quit(disk_file_name);
  let json_vol_info = JsonVolumeInfo::from(&vol);

  if json {
    println!("{}", serde_json::to_string(&json_vol_info).unwrap())
  } else {
    print_vh(json_vol_info, &vol);
  }
}

/// Formatted print of Volume Header information
fn print_vh(info: JsonVolumeInfo, vol: &OpenVolume) {
  println!("Sector size: {} bytes", info.sector_sz);
  println!("Command Tag Queueing: {} (depth {})", info.ctq_enabled, info.ctq_depth);
  println!("Root partition ID: {}", info.root_partition);
  println!("Swap partition ID: {}", info.swap_partition);

  println!();
  if let Some(boot_file) = &info.boot_file {
    println!("Boot file: {}", boot_file);
  } else {
    println!("No boot file listed.");
  }
  println!("Volume Directory:");
  print_voldir(info.vh_files);

  println!();
  println!("Partitions:");
  print_partitions(info.partitions);
  let vh = &vol.volume_header;
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
}

/// List table of files in volume directory
fn print_voldir(info: BTreeMap<usize, JsonVhFileInfo>) {
  #[derive(Tabled)]
  struct DisplayFile {
    #[header("Id")]
    id: usize,
    #[header("File Name")]
    file_name: String,
    #[header("Start Block")]
    start_block: u64,
    #[header("Size (bytes)")]
    size_bytes: u64,
    #[header("Over Length? (bytes)")]
    over_length: String,
  }

  let file_tab = info.into_iter()
    .map(|(id, file, )| DisplayFile {
      id,
      file_name: file.file_name,
      start_block: file.start_block,
      size_bytes: file.size_bytes,
      over_length: match file.over_length {
        Some(b) => format!("Yes ({})", b),
        None => "No".to_string()
      },
    })
    .collect::<Vec<DisplayFile>>();

  print!("{}", Table::new(file_tab).with(crate::table_fmt()));
}

/// Print partition table nicely
fn print_partitions(info: BTreeMap<usize, JsonPartitionInfo>) {
  #[derive(Tabled)]
  struct DisplayPartition {
    #[header("Id")]
    id: usize,
    #[header("Partition Type")]
    partition_type: String,
    #[header("Start Block")]
    start_block: u64,
    #[header("End Block")]
    end_block: u64,
    #[header("Size (blocks)")]
    size_blocks: u64,
    #[header("Over Length? (bytes)")]
    over_length: String,
  }

  let part_tab = info.into_iter()
    .map(|(id, p, )| DisplayPartition {
      id,
      partition_type: p.partition_type,
      start_block: p.start_block,
      end_block: p.end_block,
      size_blocks: p.sz_blocks,
      over_length: match p.over_length {
        Some(b) => format!("Yes ({})", b),
        None => "No".to_string()
      },
    })
    .collect::<Vec<DisplayPartition>>();

  print!("{}", Table::new(part_tab).with(crate::table_fmt()));
}

/// JSON representation of volume information
#[derive(Serialize)]
struct JsonVolumeInfo {
  sector_sz: usize,
  ctq_enabled: bool,
  ctq_depth: u8,
  root_partition: usize,
  swap_partition: usize,
  boot_file: Option<String>,
  vh_files: BTreeMap<usize, JsonVhFileInfo>,
  partitions: BTreeMap<usize, JsonPartitionInfo>,
}

impl JsonVolumeInfo {
  /// Create JsonVolumeInfo from OpenVolume
  fn from(vol: &OpenVolume) -> Self {
    let vh = &vol.volume_header;
    let file_sz = vol.disk_file_meta.len();

    let vh_files = vh.files.iter().enumerate()
      .filter(|(_id, vh_file, )| vh_file.in_use())
      .map(|(id, vh_file, )| (id, JsonVhFileInfo::from(vh_file, file_sz), ))
      .collect::<BTreeMap<usize, JsonVhFileInfo>>();

    let partitions = vh.partitions.iter().enumerate()
      .filter(|(_id, p, )| p.in_use())
      .map(|(id, p, )| (id, JsonPartitionInfo::from(p, file_sz), ))
      .collect::<BTreeMap<usize, JsonPartitionInfo>>();

    Self {
      sector_sz: vh.sector_sz,
      ctq_enabled: vh.ctq_enabled,
      ctq_depth: vh.ctq_depth,
      root_partition: vh.root_partition,
      swap_partition: vh.swap_partition,
      boot_file: vh.boot_file.clone(),
      vh_files,
      partitions,
    }
  }
}

/// JSON representation of information for one volume header file
#[derive(Serialize)]
struct JsonVhFileInfo {
  file_name: String,
  start_block: u64,
  size_bytes: u64,
  over_length: Option<u64>,
}

impl JsonVhFileInfo {
  /// Create JsonVhFileInfo from VolumeFile
  fn from(f: &VolumeFile, file_sz: u64) -> Self {
    let end_bytes = (f.block_start * sgidisklib::efs::EFS_BLOCK_SZ as u64) + f.file_sz;
    let over_length = if end_bytes > file_sz {
      Some(end_bytes - file_sz)
    } else {
      None
    };
    Self {
      file_name: match f.file_name.as_ref() {
        Some(n) => n.clone(),
        None => "".to_string()
      },
      start_block: f.block_start,
      size_bytes: f.file_sz,
      over_length,
    }
  }
}

/// JSON representation of information for one partition
#[derive(Serialize)]
struct JsonPartitionInfo {
  partition_type: String,
  start_block: u64,
  end_block: u64,
  sz_blocks: u64,
  over_length: Option<u64>,
}

impl JsonPartitionInfo {
  /// Create JsonPartitionInfo from Partition
  fn from(p: &Partition, file_sz: u64) -> Self {
    let end_block = p.block_start + p.block_sz;
    let end_byte = end_block * sgidisklib::efs::EFS_BLOCK_SZ as u64;
    let over_length = if end_byte > file_sz {
      Some(end_byte - file_sz)
    } else {
      None
    };

    JsonPartitionInfo {
      partition_type: p.partition_type.to_string(),
      start_block: p.block_start,
      end_block,
      sz_blocks: p.block_sz,
      over_length,
    }
  }
}