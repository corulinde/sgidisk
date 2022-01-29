use std::io::Read;
use std::fmt;
use std::fmt::Formatter;

use deku::prelude::*;

use crate::SgidiskLibReadError;
use crate::volhdr::raw::{VolumeDeviceParameters, VolumeDirectory};

mod raw;

/// SGI Disk Volume Header, located at the beginning of all IRIX disks
#[derive(Debug)]
pub struct SgidiskVolume {
  /// Size of disk sector in bytes
  pub sector_sz: usize,
  /// Command Tag Queueing enabled
  pub ctq_enabled: bool,
  /// Depth of Command Tag Queueing queue
  pub ctq_depth: u8,
  /// Index of root partition
  pub root_partition: usize,
  /// Index of swap partition
  pub swap_partition: usize,
  /// Array of disk partitions
  pub partitions: Vec<Partition>,
  /// Boot file name
  pub boot_file: Option<String>,
  /// Volume Directory file entries
  pub files: Vec<VolumeFile>,

  // Informational options described as "backwards compatibility only"
  pub compat_cylinders: u16,
  pub compat_heads: u16,
  pub compat_sect: u16,
  pub compat_drivecap: u32,
}

/// Partition table entry
#[derive(Debug)]
pub struct Partition {
  /// Partition type
  pub partition_type: PartitionType,
  /// Partition size, in blocks
  pub block_sz: u64,
  /// Partition offset from beginning of disk, in blocks
  pub block_start: u64,
}

/// Partition Type ID for PartitionTable
#[derive(Debug, Copy, Clone, Eq, PartialEq, DekuRead, DekuWrite)]
#[deku(type = "i32", endian = "big")]
pub enum PartitionType {
  /// Partition is volume header
  VolumeHeader = 0,
  /// 1 and 2 were used for drive types no longer supported
  Unsupported1 = 1,
  /// 1 and 2 were used for drive types no longer supported
  Unsupported2 = 2,
  /// Partition is used for data
  Raw = 3,
  /// 4 and 5 were for filesystem types we haven't ever supported on MIPS CPUs
  Unsupported4 = 4,
  /// 4 and 5 were for filesystem types we haven't ever supported on MIPS CPUs
  Unsupported5 = 5,
  /// Partition is entire volume
  EntireVolume = 6,
  /// Partition is SGI EFS
  Efs = 7,
  /// partition is part of a logical volume
  LogicalVolume = 8,
  /// Part of a "raw" logical volume
  RawLogicalVolume = 9,
  /// Partition is SGI XFS
  Xfs = 10,
  /// Partition is SGI XFS log
  XfsLog = 11,
  /// Partition is part of an XLV volume
  Xlv = 12,
  /// Partition is SGI XVM
  Xvm = 13,
  /// Partition is SGI VXVM
  Vxvm = 14,
}

/// Volume directory file entry
#[derive(Debug)]
pub struct VolumeFile {
  pub file_name: Option<String>,
  /// Starting block offset of file
  pub block_start: u64,
  /// File size (in bytes)
  pub file_sz: u64,
}

impl SgidiskVolume {
  /// Synchronously read / deserialize a SgidiskVolume
  pub fn read<R: ?Sized>(reader: &mut R) -> Result<Self, SgidiskLibReadError>
    where R: Read {
    Self::try_from(&raw::VolumeHeader::read(reader)?)
  }
}

impl Partition {
  /// Check whether a partition entry is in use, i.e. if it has a size greater
  /// than zero
  pub fn in_use(&self) -> bool {
    self.block_sz > 0
  }
}

impl TryFrom<&raw::VolumeHeader> for SgidiskVolume {
  type Error = SgidiskLibReadError;

  /// Convert from raw VolumeHeader to SgidiskVolume struct
  fn try_from(vh: &raw::VolumeHeader) -> Result<Self, Self::Error> {
    // Check and convert raw values, mostly oddly signed fields
    let root_partition = match usize::try_from(vh.vh_rootpt) {
      Ok(i) => i,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid root partition index: {}", vh.vh_rootpt)))
    };
    let swap_partition = match usize::try_from(vh.vh_swappt) {
      Ok(i) => i,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid swap partition index: {}", vh.vh_swappt)))
    };

    let ctq_enabled = vh.vh_dp.dp_flags & VolumeDeviceParameters::DP_CTQ_EN == VolumeDeviceParameters::DP_CTQ_EN;

    // Convert partition table
    let partitions = vh.vh_pt.iter()
      .map(|pt| Partition::from(pt))
      .collect();

    let boot_file = crate::bytes_to_string(&vh.vh_bootfile)?;

    // Convert volume directory entries
    let files = vh.vh_vd.iter()
      .map(|vd| VolumeFile::try_from(vd))
      .collect::<Result<Vec<VolumeFile>, SgidiskLibReadError>>()?;

    Ok(Self {
      sector_sz: vh.vh_dp.dp_secbytes as usize,
      ctq_enabled,
      ctq_depth: vh.vh_dp.dp_ctq_depth,
      root_partition,
      swap_partition,
      partitions,
      boot_file,
      files,
      compat_cylinders: vh.vh_dp.dp_cylinders,
      compat_heads: vh.vh_dp.dp_heads,
      compat_sect: vh.vh_dp.dp_sect,
      compat_drivecap: vh.vh_dp.dp_drivecap,
    })
  }
}

impl From<&raw::PartitionTable> for Partition {
  /// Convert from raw PartitionTable to Partition struct
  fn from(pt: &raw::PartitionTable) -> Self {
    Self {
      partition_type: pt.pt_type,
      block_sz: pt.pt_nblks as u64,
      block_start: pt.pt_firstlbn as u64,
    }
  }
}

impl fmt::Display for PartitionType {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    write!(f, "{:?}", self)
  }
}

impl VolumeFile {
  pub fn in_use(&self) -> bool {
    self.file_name.is_some()
  }
}

impl TryFrom<&raw::VolumeDirectory> for VolumeFile {
  type Error = SgidiskLibReadError;

  /// Convert from raw VolumeDirectory to VolumeFile struct
  fn try_from(vd: &VolumeDirectory) -> Result<Self, Self::Error> {
    let file_name = crate::bytes_to_string(&vd.vd_name)?;
    let block_start = if vd.vd_lbn == -1 {
      // Special case, older EFS systems seem to fill in w/ -1 instead of 0?
      0
    } else {
      match u64::try_from(vd.vd_lbn) {
        Ok(i) => i,
        _ => return Err(SgidiskLibReadError::Value(format!("Invalid volume directory file offset: {}", vd.vd_lbn)))
      }
    };
    let file_sz = match u64::try_from(vd.vd_nbytes) {
      Ok(i) => i,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid volume directory file size: {}", vd.vd_nbytes)))
    };

    Ok(Self {
      file_name,
      block_start,
      file_sz,
    })
  }
}