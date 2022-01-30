use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::process::exit;

use blake3;
use clap::ArgMatches;
use serde::Serialize;
use serde_json;
use sha2::{Digest, Sha256};
use tabled::{Table, Tabled};

use sgidisklib::volhdr::SgidiskVolume;

use crate::OpenVolume;

const HASH_BUF_SZ: usize = 1024 * 16;

/// Hash tool entry point
pub(crate) fn subcommand(disk_file_name: &str, cli_matches: &ArgMatches) {
  let mut vol = crate::OpenVolume::open_or_quit(disk_file_name);

  let json = cli_matches.is_present("json");
  print_hashes(&mut vol, json);
}

/// Print hashes of volume files and volumes in disk image
fn print_hashes(vol: &mut OpenVolume, json: bool) {
  let mut items = hashed_items(&vol.volume_header);

  // Fill hashes and collect/print whole image hash
  let image_hash = fill_hashes(vol, &mut items);

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

  if json {
    let json_display = JsonHashDisplay::new(image_hash, file_items, vol_items);
    println!("{}", serde_json::to_string(&json_display).unwrap());
  } else {
    let image_hash_display = ImageHashDisplayTable::from(image_hash);
    let file_hashes = HashDisplayTable::from(file_items);
    let vol_hashes = HashDisplayTable::from(vol_items);
    println!("Disk image hash:");
    image_hash_display.print();
    println!();
    println!("Volume file hashes:");
    file_hashes.print();
    println!();
    println!("Volume hashes:");
    vol_hashes.print();
  }
}

/// Fill hash data by reading over disk image, and return a hash for the whole image
fn fill_hashes(vol: &mut OpenVolume, items: &mut Vec<HashItem>) -> MultiHashResult {
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
            match items[i].hash.as_mut() {
              Some(h) => h.update(&buf[overlap]),
              _ => panic!("Missing hash entry")
            }
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

  // Finalize hashes
  items.iter_mut().for_each(|i| i.finalize());

  // Return whole image hash
  image_hash.finalize()
}

/// Compile a list of items to hash out of volume files and partitions
fn hashed_items(vh: &SgidiskVolume) -> Vec<HashItem> {
  let mut items = Vec::with_capacity(vh.partitions.len() + vh.files.len());

  // Add files
  items.append(&mut vh.files.iter()
    .filter(|f| f.in_use())
    .map(|f| {
      let start = f.block_start as i64 * sgidisklib::efs::EFS_BLOCK_SZ as i64;
      let name = f.file_name.as_ref().unwrap();
      HashItem {
        name_display: name.clone(),
        name_json: name.clone(),
        item_type: HashItemType::VolumeFile,
        start,
        end: start + f.file_sz as i64,
        hashed: 0,
        hash: Some(MultiHash::new()),
        hash_result: None,
      }
    })
    .collect::<Vec<HashItem>>());

  // Add partitions
  items.append(&mut vh.partitions.iter()
    .enumerate()
    .filter(|(_, p, )| p.in_use())
    .map(|(id, p, )| HashItem {
      name_display: format!("{:>2} ({})", id, p.partition_type),
      name_json: id.to_string(),
      item_type: HashItemType::Partition,
      start: p.block_start as i64 * sgidisklib::efs::EFS_BLOCK_SZ as i64,
      end: (p.block_start + p.block_sz) as i64 * sgidisklib::efs::EFS_BLOCK_SZ as i64,
      hashed: 0,
      hash: Some(MultiHash::new()),
      hash_result: None,
    })
    .collect::<Vec<HashItem>>());

  items.sort_by_key(|h| -h.end);

  items
}

/// JSON structure for hash display
#[derive(Serialize)]
struct JsonHashDisplay {
  image: MultiHashResult,
  volume_files: JsonHashItems,
  volumes: JsonHashItems,
}

type JsonHashItems = BTreeMap<String, JsonHashElement>;

/// JSON display entry for one hashable item
#[derive(Serialize)]
struct JsonHashElement {
  hash: MultiHashResult,
  short: Option<i64>,
}

impl JsonHashDisplay {
  /// Create a JsonHashDisplay from a whole image hash, volume files hash set, and volume hash set
  fn new(image: MultiHashResult, file_items: Vec<HashItem>, vol_items: Vec<HashItem>) -> Self {
    let volume_files = Self::items(file_items);
    let volumes = Self::items(vol_items);

    JsonHashDisplay {
      image,
      volume_files,
      volumes,
    }
  }

  /// Create a JSON tree structure from a list of HashItem objects
  fn items(items: Vec<HashItem>) -> JsonHashItems {
    items.into_iter()
      .map(|item| {
        let short = item.short_by();
        (item.name_json,
         JsonHashElement {
           hash: item.hash_result.unwrap(),
           short,
         }, )
      })
      .collect::<BTreeMap<String, JsonHashElement>>()
  }
}

/// A printable table of hashes for the entire image
#[derive(Serialize)]
struct ImageHashDisplayTable(Vec<ImageHashDisplayTableEntry>);

/// Printable image hash entry
#[derive(Tabled, Serialize)]
struct ImageHashDisplayTableEntry {
  #[header("Hash Type")]
  hash_type: &'static str,
  #[header("Hash")]
  hash_value: String,
}

impl ImageHashDisplayTable {
  /// Print formatted table to stdout
  fn print(&self) {
    print!("{}", Table::new(&self.0)
      .with(crate::table_fmt()));
  }
}

impl From<MultiHashResult> for ImageHashDisplayTable {
  /// Convert a single MultiHashResult to a printable image hash table
  fn from(h: MultiHashResult) -> Self {
    let tab = vec![
      ImageHashDisplayTableEntry {
        hash_type: "SHA-256",
        hash_value: h.sha256,
      },
      ImageHashDisplayTableEntry {
        hash_type: "BLAKE3",
        hash_value: h.blake3,
      },
    ];

    Self(tab)
  }
}

/// A printable table of hashed items
#[derive(Serialize)]
struct HashDisplayTable(Vec<HashDisplayTableEntry>);

/// Printable hashed item table entry
#[derive(Tabled, Serialize)]
struct HashDisplayTableEntry {
  #[header("Item")]
  item: String,
  #[header("Hash Type")]
  hash_type: &'static str,
  #[header("Hash")]
  hash: String,
  #[header("Short?")]
  short: String,
}

impl HashDisplayTable {
  /// Print formatted table to stdout
  fn print(&self) {
    print!("{}", Table::new(&self.0)
      .with(crate::table_fmt()));
  }
}

impl From<Vec<HashItem>> for HashDisplayTable {
  /// Convert from a list of HashItems to a printable table
  fn from(mut items: Vec<HashItem>) -> Self {
    items.sort_by(|h1, h2| h1.name_display.cmp(&h2.name_display));
    let tab = items.into_iter()
      .map(|h| {
        let short = h.short_by_str();
        let item = h.name_display;
        let hash_result = h.hash_result.unwrap();
        vec![
          HashDisplayTableEntry {
            item: item.clone(),
            hash_type: "SHA-256",
            hash: hash_result.sha256,
            short: short.clone(),
          },
          HashDisplayTableEntry {
            item,
            hash_type: "BLAKE3",
            hash: hash_result.blake3,
            short,
          },
        ]
      })
      .flatten()
      .collect::<Vec<HashDisplayTableEntry>>();

    HashDisplayTable(tab)
  }
}

/// Range based hashed item
struct HashItem {
  /// Display name of hashed item
  name_display: String,
  /// JSON name of hashed item
  name_json: String,
  /// Type of hashed item
  item_type: HashItemType,
  /// Start of hashed range (bytes)
  start: i64,
  /// End of hashed range (bytes)
  end: i64,
  /// Number of bytes hashed
  hashed: u64,
  /// Hash value tracking
  hash: Option<MultiHash>,
  /// Hash result
  hash_result: Option<MultiHashResult>,
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
#[derive(Debug, Serialize)]
pub(crate) struct MultiHashResult {
  pub(crate) blake3: String,
  pub(crate) sha256: String,
}

impl HashItem {
  fn finalize(&mut self) {
    let hash = self.hash.take().unwrap();
    self.hash_result = Some(hash.finalize());
  }

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
      Some(n) => format!("{} bytes!", n)
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