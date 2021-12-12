use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::process::exit;

use blake3;
use clap::ArgMatches;
use sha2::{Digest, Sha256};
use tabled::{Tabled, Table};

use sgidisklib::volhdr::SgidiskVolume;
use crate::OpenVolume;

const HASH_BUF_SZ: usize = 4096;

/// Hash tool entry point
pub(crate) fn subcommand(disk_file_name: &str, _cli_matches: &ArgMatches) {
  let mut vol = crate::OpenVolume::open_or_quit(disk_file_name);

  print_hashes(&mut vol);
}

/// Print hashes of volume files and volumes in disk image
fn print_hashes(vol: &mut OpenVolume) {
  let mut items = hashed_items(&vol.volume_header);

  // Fill hashes and collect/print whole image hash
  let image_hash = fill_hashes(vol, &mut items);
  let image_hash = image_hash.finalize();
  print_image_hash(image_hash);

  // Sort hashable items into files and volumes and collect/print hashes
  let (file_items, vol_items) = items.into_iter()
    .fold((Vec::new(), Vec::new(), ),
          |(mut file_items, mut vol_items, ), h| {
            match &h.item_type {
              HashItemType::VolumeFile => file_items.push(h),
              HashItemType::Partition => vol_items.push(h)
            }
            (file_items, vol_items, )
          });
  println!();
  print_vol_hashes(vol_items);
  println!();
  print_file_hashes(file_items);
}

/// Print hash of whole disk image
fn print_image_hash(h: MultiHashResult) {
  #[derive(Tabled)]
  struct ImageHash {
    hash_type: &'static str,
    hash_value: String,
  }

  let tab = vec![
    ImageHash {
      hash_type: "SHA-256",
      hash_value: h.sha256,
    },
    ImageHash {
      hash_type: "BLAKE3",
      hash_value: h.blake3,
    },
  ];

  println!("Disk image hash:");
  print!("{}", Table::new(tab)
    .with(crate::table_fmt()));
}

/// Print hashes of volumes
fn print_vol_hashes(mut items: Vec<HashItem>) {
  #[derive(Tabled)]
  struct PartitionHash {
    partition: String,
    hash_type: &'static str,
    hash: String,
    short: String,
  }

  items.sort_by(|h1, h2| h1.name.cmp(&h2.name));
  let tab = items.into_iter()
    .map(|h| {
      let short = h.short_by_str();
      let partition = h.name;
      let h = h.hash.finalize();
      vec![
        PartitionHash {
          partition: partition.clone(),
          hash_type: "SHA-256",
          hash: h.sha256,
          short: short.clone(),
        },
        PartitionHash {
          partition,
          hash_type: "BLAKE3",
          hash: h.blake3,
          short,
        },
      ]
    })
    .flatten()
    .collect::<Vec<PartitionHash>>();

  println!("Partition hashes:");
  print!("{}", Table::new(tab)
    .with(crate::table_fmt()));
}

/// Print hashes of volume files
fn print_file_hashes(mut items: Vec<HashItem>) {
  #[derive(Tabled)]
  struct FileHash {
    file: String,
    hash_type: &'static str,
    hash: String,
    short: String,
  }

  items.sort_by(|h1, h2| h1.name.cmp(&h2.name));
  let tab = items.into_iter()
    .map(|h| {
      let short = h.short_by_str();
      let file = h.name;
      let h = h.hash.finalize();
      vec![
        FileHash {
          file: file.clone(),
          hash_type: "SHA-256",
          hash: h.sha256,
          short: short.clone(),
        },
        FileHash {
          file,
          hash_type: "BLAKE3",
          hash: h.blake3,
          short,
        },
      ]
    })
    .flatten()
    .collect::<Vec<FileHash>>();

  println!("Volume file hashes:");
  print!("{}", Table::new(tab)
    .with(crate::table_fmt()));
}

/// Fill hash data by reading over disk image, and return a hash for the whole image
fn fill_hashes(vol: &mut OpenVolume, items: &mut Vec<HashItem>) -> MultiHash {
  let len = items.len();
  let mut finished = vec![false; len];

  // Return to beginning of file
  if let Err(e) = vol.disk_file.seek(SeekFrom::Start(0)) {
    eprintln!("Failed to seek: {:?}", &e);
    exit(crate::exit_codes::IO_ERR);
  }
  let mut pos = 0u64;

  // Read entire image in chunks
  let mut image_hash = MultiHash::new();
  let mut fh = &vol.disk_file;
  let mut buf = [0u8; HASH_BUF_SZ];
  loop {
    match fh.read(&mut buf) {
      // End of file
      Ok(0) => break,

      // Successful read
      Ok(n) => {
        // Update whole file hash
        image_hash.update(&buf[0..n]);

        // Read window from pos to end
        let end = pos + n as u64;

        // For each hashable item...
        for i in 0..len {
          // Skip completed hashes
          if finished[i] {
            continue;
          }
          // If we have moved past its end, mark it complete
          if (items[i].end as u64) < pos {
            finished[i] = true;
            continue;
          }
          // If we have overlap...
          if let Some(overlap) = items[i].window_overlap(pos as i64, end as i64) {
            // Update the item's hash with the overlapping bytes
            items[i].hashed += (overlap.end - overlap.start) as u64;
            items[i].hash.update(&buf[overlap]);
          }
        }

        pos = end;
      }

      // IO error
      Err(e) => {
        eprintln!("Error while reading disk image: {:?}", &e);
        exit(crate::exit_codes::IO_ERR);
      }
    }
  }

  // Return whole image hash
  image_hash
}

/// Compile a list of items to hash out of volume files and partitions
fn hashed_items(vh: &SgidiskVolume) -> Vec<HashItem> {
  let mut items = Vec::with_capacity(vh.partitions.len() + vh.files.len());

  // Add files
  items.append(&mut vh.files.iter()
    .filter(|f| f.in_use())
    .map(|f| {
      let start = f.block_start as i64 * sgidisklib::efs::EFS_BLOCK_SZ as i64;
      HashItem {
        name: f.file_name.as_ref().unwrap().clone(),
        item_type: HashItemType::VolumeFile,
        start,
        end: start + f.file_sz as i64,
        hashed: 0,
        hash: MultiHash::new(),
      }
    })
    .collect::<Vec<HashItem>>());

  // Add partitions
  items.append(&mut vh.partitions.iter()
    .enumerate()
    .filter(|(_, p, )| p.in_use())
    .map(|(id, p, )| HashItem {
      name: format!("{:>2} ({})", id, p.partition_type),
      item_type: HashItemType::Partition,
      start: p.block_start as i64 * sgidisklib::efs::EFS_BLOCK_SZ as i64,
      end: (p.block_start + p.block_sz) as i64 * sgidisklib::efs::EFS_BLOCK_SZ as i64,
      hashed: 0,
      hash: MultiHash::new(),
    })
    .collect::<Vec<HashItem>>());

  items.sort_by_key(|h| -h.end);

  items
}

/// Range based hashed item
struct HashItem {
  /// Name of hashed item
  name: String,
  /// Type of hashed item
  item_type: HashItemType,
  /// Start of hashed range (bytes)
  start: i64,
  /// End of hashed range (bytes)
  end: i64,
  /// Number of bytes hashed
  hashed: u64,
  /// Hash value tracking
  hash: MultiHash,
}

#[derive(Debug, Copy, Clone)]
enum HashItemType {
  Partition,
  VolumeFile,
}

/// Hashes with BLAKE2b, SHA-256
pub(crate) struct MultiHash {
  blake3: blake3::Hasher,
  sha256: Sha256,
}

/// Results from MultiHash hashes
#[derive(Debug)]
pub(crate) struct MultiHashResult {
  pub(crate) blake3: String,
  pub(crate) sha256: String,
}

impl HashItem {
  /// Determine the overlap of our hashed item window into a supplied buffer window, as a range of bytes
  fn window_overlap(&self, start: i64, end: i64) -> Option<Range<usize>> {
    // No overlap case
    if self.end <= start || self.start >= end {
      return None;
    }

    // Overlap start into block
    let ovr_start = if self.start > start {
      self.start - start
    } else {
      0
    } as usize;
    // Overlap end into block
    let ovr_end = (self.end.min(end) - start) as usize;

    Some(ovr_start..ovr_end)
  }

  /// Determine whether we're short on bytes hashed
  fn short_by(&self) -> Option<i64> {
    let sz = self.end - self.start;
    let hashed = self.hashed as i64;
    if hashed != sz {
      Some(sz - hashed)
    } else {
      None
    }
  }

  /// Return a convenient table string based on short_by()
  fn short_by_str(&self) -> String {
    match self.short_by() {
      None => "No".to_string(),
      Some(n) => format!("Short {} bytes!", n)
    }
  }
}

impl MultiHash {
  /// Create a new MultiHash hasher
  pub fn new() -> Self {
    let blake3 = blake3::Hasher::new();
    let sha256 = Sha256::new();

    MultiHash {
      blake3,
      sha256,
    }
  }

  /// Update hash with data
  pub fn update(&mut self, b: &[u8]) {
    self.blake3.update(b);
    self.sha256.update(b);
  }

  /// Finalize hash and populate results
  pub fn finalize(self) -> MultiHashResult {
    MultiHashResult {
      blake3: Self::bytes_to_hex(self.blake3.finalize().as_bytes()),
      sha256: Self::bytes_to_hex(&self.sha256.finalize()[..]),
    }
  }

  /// Format byte slice as hex, perhaps somewhat inefficiently
  fn bytes_to_hex(b: &[u8]) -> String {
    b.iter()
      .map(|b| format!("{:02X}", b))
      .collect::<Vec<String>>()
      .concat()
  }
}