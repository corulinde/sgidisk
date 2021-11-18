use std::io::{Read, Seek};
use std::io::SeekFrom;

use deku::prelude::*;

use crate::SgidiskLibReadError;

/// Structure of the super-block for the extent filesystem
#[derive(Debug, DekuRead, DekuWrite)]
pub(crate) struct EfsSuperblock {
  /// Size of filesystem, in sectors
  #[deku(endian = "big")]
  pub(crate) fs_size: i32,
  /// Basic Block (BB) offset to first cylinder group
  #[deku(endian = "big")]
  pub(crate) fs_firstcg: i32,
  /// Size of cylinder group in BB's
  #[deku(endian = "big")]
  pub(crate) fs_cgfsize: i32,
  /// BB's of inodes per cylinder group
  #[deku(endian = "big")]
  pub(crate) fs_cgisize: i16,
  /// Sectors per track
  #[deku(endian = "big")]
  pub(crate) fs_sectors: i16,
  /// Heads per cylinder
  #[deku(endian = "big")]
  pub(crate) fs_heads: i16,
  /// Number of cylinder groups in filesystem
  #[deku(endian = "big")]
  pub(crate) fs_ncg: i16,
  /// Fs needs to be FSCK'd
  pub(crate) fs_dirty: EfsSuperblockDirty,
  /// Last super-block update
  #[deku(endian = "big", pad_bytes_before = "2")]
  pub(crate) fs_time: i32,
  /// Magic number
  pub(crate) fs_magic: EfsSuperblockMagic,
  /// File system name
  pub(crate) fs_fname: [u8; 6],
  /// File system pack name
  pub(crate) fs_fpack: [u8; 6],
  /// Size of bitmap in bytes
  #[deku(endian = "big")]
  pub(crate) fs_bmsize: i32,
  /// Total free data blocks
  #[deku(endian = "big")]
  pub(crate) fs_tfree: i32,
  /// Total free inodes
  #[deku(endian = "big")]
  pub(crate) fs_tinode: i32,
  /// Bitmap location
  #[deku(endian = "big")]
  pub(crate) fs_bmblock: i32,
  /// Location of replicated superblock
  #[deku(endian = "big")]
  pub(crate) fs_replsb: i32,
  /// Last allocated inode
  #[deku(endian = "big")]
  pub(crate) fs_lastialloc: i32,
  /// Space for expansion - MUST BE ZERO
  pub(crate) fs_spare: [u8; 20],
  /// Checksum of volume portion of FS
  #[deku(endian = "big")]
  pub(crate) fs_checksum: i32,
}

impl EfsSuperblock {
  /// Size of the EFS Superblock in bytes
  const SIZE: usize = 92;
}

/// Values for fs_dirty. If a filesystem was cleanly unmounted, and started
/// clean before being mounted then fs_dirty will be Clean. Otherwise the
/// filesystem is suspect in one of several ways. If it was a root filesystem
/// and had to be mounted even though it was dirty, the fs_dirty flag gets
/// set to ActiveDirty so that user level tools know to clean the root
/// filesystem. If the filesystem was clean and is mounted, then the fs_dirty
/// flag gets set to Active. Dirty is a particular value to assign fs_dirty to
/// when a filesystem is known to be dirty.
#[derive(Debug, Copy, Clone, Eq, PartialEq, DekuRead, DekuWrite)]
#[deku(type = "i16", endian = "big")]
pub(crate) enum EfsSuperblockDirty {
  /// Unmounted && clean
  #[deku(id = "0x0000i16")]
  Clean,
  /// Mounted a dirty fs (root only)
  #[deku(id = "0x0BADi16")]
  ActiveDirty,
  /// Mounted && clean
  #[deku(id = "0x7777i16")]
  Active,
  /// Random value for dirtiness
  #[deku(id = "0x1234i16")]
  Dirty,
}

/// Magic number of EFS superblock
#[derive(Debug, Copy, Clone, Eq, PartialEq, DekuRead, DekuWrite)]
#[deku(type = "i32", endian = "big")]
pub(crate) enum EfsSuperblockMagic {
  /// Pre-IRIX 3.3 compatible?
  #[deku(id = "0x00072959i32")]
  OldMagic,
  /// IRIX 3.3 (and up?) filesystems need a new magic number to ensure there's
  /// no attempt to (disasterously!) use them on a pre-3.3 system.
  #[deku(id = "0x0007295ai32")]
  NewMagic,
}

impl EfsSuperblock {
  /// Find superblock on disk assuming we started at the beginning of the partition
  fn seek_superblock<R: ?Sized>(reader: &mut R) -> Result<(), SgidiskLibReadError>
    where R: Seek {
    // "Basic block 0 is unused and is available to contain a bootstrap
    // program or other information. Basic block 1 is the superblock."
    // Therefore, seek one block forward before reading...
    match reader.seek(SeekFrom::Current(super::EFS_BLOCK_SZ as i64)) {
      Ok(_) => Ok(()),
      Err(e) => Err(SgidiskLibReadError::Io(e))
    }
  }

  /// Parse byte slice into EfsSuperblock struct
  fn parse_superblock(buf: &[u8]) -> Result<Self, SgidiskLibReadError> {
    let (_, sb, ) = Self::from_bytes((buf, 0, ))?;
    Ok(sb)
  }

  /// Synchronously read an EFS Superblock
  pub(crate) fn read<R: ?Sized>(reader: &mut R) -> Result<Self, SgidiskLibReadError>
    where R: Read + Seek
  {
    Self::seek_superblock(reader)?;

    // Read superblock
    let mut buf = vec![0; Self::SIZE];
    reader.read_exact(&mut buf)?;
    Self::parse_superblock(&buf)
  }
}