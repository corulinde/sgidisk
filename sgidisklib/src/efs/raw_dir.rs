use std::io::Read;

use deku::prelude::*;

use crate::SgidiskLibReadError;

/// One block of directory data in an EFS inode.
///
/// A dirblk is composed of 3 major components: a header, entry offsets and
/// entries.  Initially, a dirblock is all zeros, except for the magic number
/// and the freeoffset.  A entries are allocated, a byte is reserved at the
/// beginning of the "space" array for holding the offset to the entry. At the
/// end of the "space" array the actual entry is stored.  The directory is
/// considered full, if a name is going to be added and (1) there is not enough
///  room for the dent plus halfword alignment padding plus the byte offset.
///
/// The directory management procedures that return an "offset" actually return
/// a magic cookie with the following format:
/// directory-block-number<23:0>|index-into-offsets<7:0>
#[derive(Debug, DekuRead, DekuWrite)]
// "moo" - IRIX efs_dir.h
#[deku(magic = b"\xBE\xEF")]
pub(crate) struct DirectoryBlock {
  /// Offset to first used dent byte
  pub(crate) firstused: u8,
  /// Number of offset slots in DirectoryBlock
  pub(crate) slots: u8,
  /// Space for efs_dent's
  pub(crate) space: [u8; Self::SPACE_SZ],
}

impl DirectoryBlock {
  /// Size of a DirectoryBlock in bytes (one EFS block)
  pub(crate) const SIZE: usize = super::EFS_BLOCK_SZ;
  /// Size of header (start of block without payload area)
  const HEADER_SZ: usize = 4;
  /// Size of DirectoryEntry payload in bytes
  const SPACE_SZ: usize = Self::SIZE - 4;
  /// Theoretical maximum number of entries
  const MAX_ENTRIES: usize = Self::SPACE_SZ / DirectoryEntry::MIN_SIZE;
}

/// Entry structure
#[derive(Debug, DekuRead, DekuWrite)]
pub(crate) struct DirectoryEntry {
  /// Inode number
  #[deku(endian = "big")]
  pub(crate) inode: u32,
  /// Length of string in d_name
  pub(crate) d_namelen: u8,
  /// Name "flex array"
  #[deku(count = "d_namelen")]
  pub(crate) d_name: Vec<u8>,
}

impl DirectoryEntry {
  /// Each entry is at least:
  /// starting area: 1 byte offset
  /// ending: 4 byte inode + 1 byte strlen + 1 byte name
  /// then, padded to 2 byte half word
  const MIN_SIZE: usize = 8;
}

impl DirectoryBlock {
  /// Parse byte buffer into DirectoryBlock
  fn parse_directory_block(buf: &[u8]) -> Result<Self, SgidiskLibReadError> {
    let (_, db, ) = Self::from_bytes((buf, 0, ))?;
    Ok(db)
  }

  /// Synchronously read a DirectoryBlock
  pub(crate) fn read<R: ?Sized>(reader: &mut R) -> Result<Self, SgidiskLibReadError>
    where R: Read
  {
    let mut buf = vec![0; super::EFS_BLOCK_SZ];
    reader.read_exact(&mut buf)?;
    Self::parse_directory_block(&buf)
  }

  /// Get directory entries from a DirectoryBlock
  pub(crate) fn dir_entries(&self) -> Result<Vec<DirectoryEntry>, SgidiskLibReadError> {
    // Perform some sanity checking
    let slots = self.slots as usize;
    if slots > DirectoryBlock::MAX_ENTRIES {
      return Err(SgidiskLibReadError::Value(format!("Directory block listed more than maximum allowed number of entries: {}", slots)));
    }

    let mut entries = Vec::with_capacity(self.slots as usize);

    // For each directory entry
    for slot in 0..slots {
      // Calculate offset to directory entry structure and sanity check
      let compact_offset = self.space[slot] as usize;
      if compact_offset < DirectoryBlock::HEADER_SZ >> 1 {
        return Err(SgidiskLibReadError::Bounds(format!("Directory entry offset is prior to payload area (compact {})", compact_offset)));
      }
      // Apparently the "slot" offset data is compacted by shifting it right one before storage and applies from the start of the block
      // See efs_dir.h EFS_COMPACT, EFS_REALOFF, etc. "firstused" seems to not apply as an offset...
      let offset = ((self.space[slot] as usize) << 1) - DirectoryBlock::HEADER_SZ;
      if offset >= DirectoryBlock::SPACE_SZ {
        return Err(SgidiskLibReadError::Bounds(format!("Directory entry offset is past end of payload, at {}", offset)));
      }
      // Parse DirectoryEntry and add to list
      let buf = &self.space[offset..];
      let (_, dent, ) = DirectoryEntry::from_bytes((buf, 0, ))?;
      entries.push(dent);
    }

    Ok(entries)
  }
}