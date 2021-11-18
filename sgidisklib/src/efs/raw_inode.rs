use std::io::Read;

use deku::prelude::*;

use crate::SgidiskLibReadError;

/// Extent based filesystem inode as it appears on disk. The efs inode is
/// exactly 128 bytes long.
#[derive(Debug, DekuRead, DekuWrite)]
pub(crate) struct EfsInode {
  /// Mode and type of file
  #[deku(endian = "big")]
  pub(crate) di_mode: u16,
  /// Number of links to file
  #[deku(endian = "big")]
  pub(crate) di_nlink: i16,
  /// Owner's user id
  #[deku(endian = "big")]
  pub(crate) di_uid: u16,
  /// Owner's group id
  #[deku(endian = "big")]
  pub(crate) di_gid: u16,
  /// Number of bytes in file
  #[deku(endian = "big")]
  pub(crate) di_size: i32,
  /// Time last accessed
  #[deku(endian = "big")]
  pub(crate) di_atime: i32,
  /// Time last modified
  #[deku(endian = "big")]
  pub(crate) di_mtime: i32,
  /// Time created
  #[deku(endian = "big")]
  pub(crate) di_ctime: i32,
  /// Generation number
  #[deku(endian = "big")]
  pub(crate) di_gen: u32,
  /// Number of extents
  #[deku(endian = "big")]
  pub(crate) di_numextents: i16,
  /// Version of inode
  pub(crate) di_version: u8,
  /// Spare - used by AFS
  pub(crate) di_spare: u8,
  /// Union struct of extent or dev data
  pub(crate) data: [u8; Self::EXTENT_DATA_AREA_SZ],
}

impl EfsInode {
  /// File mode mask
  pub(crate) const INODE_MODE_MASK: u16 = 0x07777;
  /// File types (inode formats)
  pub(crate) const INODE_TYPE_MASK: u16 = 0o170000;
  /// FIFO queue
  pub(crate) const INODE_TYPE_FIFO: u16 = 0o010000;
  /// Character special
  pub(crate) const INODE_TYPE_FCHR: u16 = 0o020000;
  /// Character special link
  pub(crate) const INODE_TYPE_FCHRLNK: u16 = 0o030000;
  /// Directory
  pub(crate) const INODE_TYPE_DIR: u16 = 0o040000;
  /// Block special
  pub(crate) const INODE_TYPE_BLK: u16 = 0o060000;
  /// Block special link
  pub(crate) const INODE_TYPE_BLKLNK: u16 = 0o070000;
  /// Regular
  pub(crate) const INODE_TYPE_REG: u16 = 0o100000;
  /// Symbolic link
  pub(crate) const INODE_TYPE_LNK: u16 = 0o120000;
  /// Socket
  pub(crate) const INODE_TYPE_SOCK: u16 = 0o140000;

  /// Size of inode in bytes
  pub(crate) const SIZE: usize = 128;

  /// Size of extents data area (union struct) in bytes
  pub(crate) const EXTENT_DATA_AREA_SZ: usize = 96;

  /// Number of directly mappable extents (also in fact number of possible
  /// indirect extents since these live in the direct extent table).
  pub(crate) const EFS_DIRECTEXTENTS: usize = 12;
}

/// Layout of an extent, in memory and on disk. This structure is laid out to
/// take exactly 8 bytes.
///
/// "Magic number MUST BE ZERO"
#[derive(Debug, DekuRead, DekuWrite)]
#[deku(magic = b"\x00")]
pub(crate) struct Extent {
  /// Basic block number
  #[deku(endian = "big", bits = "24")]
  pub(crate) ex_bn: u32,
  /// Length of this extent, in BB's
  pub(crate) ex_length: u8,
  /// Logical BB offset into file, or, total number of indirect extents
  #[deku(endian = "big", bits = "24")]
  pub(crate) ex_offset: u32,
}

impl Extent {
  /// Size of an Extent, in bytes
  pub(crate) const SIZE: usize = 8;
  /// Therefore follows as the night the day a computable number for the
  /// maximum number of extents possible for an inode.  Unfortunately,
  /// since i_numextents is a signed short, the real value for numextents
  /// is MIN(32767, ((EFS_DIRECTEXTENTS * EFS_MAXINDIRBBS * BBSIZE) /
  ///                sizeof(struct extent))
  /// In an ideal world, we would change numextents to an unsigned short,
  /// but we're a little pressed for time at this point, so we're just
  /// going to leave it.  If you decide to change the type of numextents,
  /// you should check fsck and its ilk to ensure that they do the right thing.
  /// -- IRIX efs_ino.h
  pub(crate) const MAX_EXTENTS: usize = 32767;
}

impl EfsInode {
  /// Unpack a byte slice into a raw EfsInode struct
  fn parse_inode(buf: &[u8]) -> Result<Self, SgidiskLibReadError> {
    let (_, inode, ) = Self::from_bytes((buf, 0, ))?;
    Ok(inode)
  }

  /// Synchronously read / deserialize an EfsInode
  pub(crate) fn read<R: ?Sized>(reader: &mut R) -> Result<Self, SgidiskLibReadError>
    where R: Read {
    let mut buf = vec![0; Self::SIZE];
    reader.read_exact(&mut buf)?;
    Self::parse_inode(&buf)
  }
}

impl Extent {
  /// Unpack a byte slice into a raw Extent struct
  fn parse_extent(buf: &[u8]) -> Result<Self, SgidiskLibReadError> {
    let (_, extent, ) = Self::from_bytes((buf, 0, ))?;
    Ok(extent)
  }

  /// Parse one or more extents out of a byte buffer
  pub(crate) fn parse_extents(buf: &[u8]) -> Result<Vec<Extent>, SgidiskLibReadError> {
    // Check buffer length against extent size
    let buf_len = buf.len();
    if buf_len % Extent::SIZE != 0 {
      return Err(SgidiskLibReadError::Value(format!("Extent area ({}) is not a multiple of Extent structure size", buf_len)));
    }
    buf.chunks(Extent::SIZE).map(Self::parse_extent).collect()
  }
}