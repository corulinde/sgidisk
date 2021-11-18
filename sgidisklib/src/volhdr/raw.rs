use std::io::Read;

use deku::prelude::*;

use crate::SgidiskLibReadError;

/// Format for volume header information
///
/// The volume header is a block located at the beginning of all disk
/// media (sector 0).  It contains information pertaining to physical
/// device parameters and logical partition information.
///
/// The volume header is manipulated by disk formatters/verifiers,
/// partition builders (e.g. fx, dvhtool, and mkfs), and disk drivers.
///
/// Previous versions of IRIX wrote a copy of the volume header
/// located at sector 0 of each track of cylinder 0.  These copies were
/// never used, and reduced the capacity of the volume header to hold large
/// files, so this practice was discontinued.
///
/// The volume header is constrained to be less than or equal to 512
/// bytes long.  A particular copy is assumed valid if no drive errors
/// are detected, the magic number is correct, and the 32 bit 2's complement
/// of the volume header is correct.  The checksum is calculated by initially
/// zeroing vh_csum, summing the entire structure and then storing the
/// 2's complement of the sum.  Thus a checksum to verify the volume header
/// should be 0.
///
/// The error summary table, bad sector replacement table, and boot blocks are
/// located by searching the volume directory within the volume header.
///
/// Tables are sized simply by the integral number of table records that
/// will fit in the space indicated by the directory entry.
///
/// The amount of space allocated to the volume header, replacement blocks
/// and other tables is user defined when the device is formatted.
#[derive(Debug, DekuRead, DekuWrite)]
#[deku(magic = b"\x0B\xE5\xA9\x41")]
pub(crate) struct VolumeHeader {
  /// Root partition number
  #[deku(endian = "big")]
  pub(crate) vh_rootpt: i16,
  /// Swap partition number
  #[deku(endian = "big")]
  pub(crate) vh_swappt: i16,
  /// Name of file to boot
  pub(crate) vh_bootfile: [u8; Self::BOOTF_NAME_SZ],
  /// Device parameters
  pub(crate) vh_dp: VolumeDeviceParameters,
  /// Other vol hdr contents
  pub(crate) vh_vd: [VolumeDirectory; Self::N_VOL_DIR],
  /// Device partition layout
  pub(crate) vh_pt: [PartitionTable; Self::N_PAR_TAB],
  /// Volume header checksum
  #[deku(endian = "big", pad_bytes_after = "4")]
  pub(crate) vh_csum: i32,
}

impl VolumeHeader {
  /// On-disk size of VolumeHeader in bytes
  const SIZE: usize = 512;

  /// 16 unix partitions
  pub(crate) const N_PAR_TAB: usize = 16;
  /// Max of 15 directory entries
  pub(crate) const N_VOL_DIR: usize = 15;
  /// Max 16 chars in boot file name
  const BOOTF_NAME_SZ: usize = 16;
}

/// Device parameters are in the volume header to determine mapping from
/// logical block numbers to physical device addresses alignment of fields
/// has to remain as it used to be, so old drive headers still match.
#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub(crate) struct VolumeDeviceParameters {
  #[deku(pad_bytes_before = "4")]
  /// Backwards compat only, so older prtvtoc, fx, etc. don't have problems
  /// when drives moved around. Don't count it being filled in in the future.
  /// It and dp_heads, dp_sect are deliberately named differently than the old
  /// fields in their positions
  pub(crate) dp_cylinders: u16,
  #[deku(pad_bytes_before = "2")]
  /// Backwards compatibility only
  pub(crate) dp_heads: u16,
  /// Depth of CTQ queue
  pub(crate) dp_ctq_depth: u8,
  #[deku(pad_bytes_before = "3")]
  /// Backwards compatibility only
  pub(crate) dp_sect: u16,
  /// Length of sector in bytes
  pub(crate) dp_secbytes: u16,
  #[deku(pad_bytes_before = "2")]
  /// Flags used by disk driver
  pub(crate) dp_flags: i32,
  #[deku(pad_bytes_before = "20")]
  /// Drive capacity in blocks; this is in a field that was never used for SCSI
  /// drives prior to IRIX 6.3, so it will often be zero. When found to be zero,
  /// or whenever drive capacity changes, this is reset by fx; programs should
  /// not rely on this being non-zero, since older drives might well never have
  /// had this newer fx run on them.
  pub(crate) dp_drivecap: u32,
}

/// Boot blocks, bad sector tables, and the error summary table, are located
/// via the volume_directory.
#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub(crate) struct VolumeDirectory {
  /// Name
  pub(crate) vd_name: [u8; Self::VDNAME_SZ],
  /// Logical block number
  pub(crate) vd_lbn: i32,
  /// File length in bytes
  pub(crate) vd_nbytes: i32,
}

impl VolumeDirectory {
  const VDNAME_SZ: usize = 8;
}

/// Partition table describes logical device partitions (device drivers examine
/// this to determine mapping from logical units to cylinder groups, device
/// formatters/verifiers examine this to determine location of replacement
/// tracks/sectors, etc.)
///
/// NOTE: pt_firstlbn SHOULD BE CYLINDER ALIGNED
#[derive(Debug, DekuRead, DekuWrite)]
pub(crate) struct PartitionTable {
  /// Number of logical blocks in partition
  #[deku(endian = "big")]
  pub(crate) pt_nblks: u32,
  /// First logical block of partition
  #[deku(endian = "big")]
  pub(crate) pt_firstlbn: u32,
  /// Use of partition
  pub(crate) pt_type: super::PartitionType,
}

impl VolumeHeader {
  /// Parse byte slice into VolumeHeader struct
  fn parse_volume_header(buf: &[u8]) -> Result<Self, SgidiskLibReadError> {
    let (_, vh, ) = Self::from_bytes((buf, 0, ))?;
    Ok(vh)
  }

  /// Synchronously read / deserialize a VolumeHeader
  pub(crate) fn read<R: ?Sized>(reader: &mut R) -> Result<Self, SgidiskLibReadError>
    where R: Read
  {
    let mut buf = vec![0; Self::SIZE];
    reader.read_exact(&mut buf)?;
    Self::parse_volume_header(&buf)
  }
}