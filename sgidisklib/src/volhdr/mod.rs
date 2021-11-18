use std::io::Read;

use deku::prelude::*;

use crate::SgidiskLibReadError;

mod raw;

/// SGI Disk Volume Header, located at the beginning of all IRIX disks
#[derive(Debug)]
pub struct SgidiskVolume {
  /// Size of disk sector in bytes
  pub sector_sz: u64,
  /// Index of root partition
  pub root_partition: usize,
  /// Index of swap partition
  pub swap_partition: usize,
  /// Array of disk partitions
  pub partitions: Vec<Partition>,
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

    // Convert partition table
    let partitions = vh.vh_pt.iter()
      .map(|pt| Partition::from(pt))
      .collect();

    Ok(Self {
      sector_sz: vh.vh_dp.dp_secbytes as u64,
      root_partition,
      swap_partition,
      partitions,
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