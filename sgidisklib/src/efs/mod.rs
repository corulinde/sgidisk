use std::cmp::min;
use std::io::{Read, Seek, SeekFrom};

use chrono::{DateTime, Local, TimeZone};

use crate::SgidiskLibReadError;

mod raw_sb;
mod raw_inode;
mod raw_dir;

pub mod dir;

/// Canonical "Basic Block" size of everything in EFS
pub const EFS_BLOCK_SZ: usize = 512;

/// Information about an in-file EFS filesystem
#[derive(Debug)]
pub struct Efs {
  /// Length of sector, in bytes (from SgidiskVolume)
  pub sector_sz: u64,
  /// Starting byte of the EFS partition within the current file
  pub partition_start: u64,
  /// Total size of the EFS filesystem in bytes
  pub size: u64,
  /// Offset to the start of cylinder groups (in Basic Blocks)
  pub cg_start: u64,
  /// Size of cylinder group (in Basic Blocks)
  pub cg_size: u64,
  /// Number of inodes per cylinder group
  pub cg_inodes: u64,
  /// Number of cylinder groups in the filesystem
  pub cg_count: u64,
}

/// Inode, representing an entry in the filesystem
#[derive(Debug)]
pub struct Inode {
  /// Type of inode
  pub inode_type: InodeType,
  /// Unix mode of entry
  pub unix_mode: u16,
  /// User ID of entry's owner
  pub owner_uid: u16,
  /// Group ID of entry's owner
  pub owner_gid: u16,
  /// Size of file in bytes
  pub size: u64,
  /// Creation time
  pub ctime: DateTime<chrono::Local>,
  /// Modification time
  pub mtime: DateTime<chrono::Local>,
  /// Access time
  pub atime: DateTime<chrono::Local>,
  /// Number of extents
  pub num_extents: usize,
  /// Extents, if not dev type
  pub(crate) extents: Vec<raw_inode::Extent>,
}

/// Inode type
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum InodeType {
  /// FIFO queue
  Fifo,
  /// Character device
  CharacterSpecial,
  /// Character device link
  CharacterSpecialLink,
  /// Directory
  Directory,
  /// Block device
  BlockSpecial,
  /// Block device link
  BlockSpecialLink,
  /// Regular file
  RegularFile,
  /// Symbolic link
  SymbolicLink,
  /// Socket
  Socket,
}

impl Efs {
  /// Check that a read from an absolute offset is within the bounds of the filesystem
  pub(crate) fn check_read_absolute(&self, start: u64, len: u64) -> Result<(), SgidiskLibReadError> {
    if start < self.partition_start {
      return Err(SgidiskLibReadError::Bounds(format!("Read at {} starts before beginning of filesystem ({})", start, self.partition_start)));
    }
    if start + len > self.partition_start + self.size {
      return Err(SgidiskLibReadError::Bounds(format!("Read at {} for {} bytes goes past end of filesystem", self.partition_start + start, len)));
    }

    Ok(())
  }

  /// Check that a read from a numbered block is within the bounds of the filesystem
  pub(crate) fn check_read_block(&self, start_block: u64, len: u64) -> Result<(), SgidiskLibReadError> {
    let start = self.partition_start + start_block * EFS_BLOCK_SZ as u64;
    self.check_read_absolute(start, len)
  }

  /// Relative offset of start of cylinder group from start of partition
  fn cg_start_rel(&self, cg: u64) -> Option<u64> {
    // Check that the provided CG is not past end of FS
    if cg >= self.cg_count {
      return None;
    }
    // Calculate relative offset of CG, not considering start of partition
    let rel_start = (self.cg_start + cg * self.cg_size) * EFS_BLOCK_SZ as u64;
    // Bounds check versus FS size
    if rel_start as u64 > self.size {
      None
    } else {
      Some(rel_start)
    }
  }

  /// Relative offset of inode from start of partition
  fn inode_start_rel(&self, inode: u64) -> Option<u64> {
    // Cylinder group of inode
    let cg = inode / self.cg_inodes;
    // Offset of cylinder group
    let cg_start = self.cg_start_rel(cg)?;
    // Offset of inode in cylinder group
    let inode_off = (inode % self.cg_inodes) * raw_inode::EfsInode::SIZE as u64;
    Some(cg_start + inode_off)
  }

  /// Errored absolute offset of inode from start of partiton
  fn inode_start(&self, inode: u64) -> Result<u64, SgidiskLibReadError> {
    if let Some(offset_rel) = self.inode_start_rel(inode) {
      Ok(self.partition_start + offset_rel)
    } else {
      Err(SgidiskLibReadError::Bounds(format!("Inode {} has invalid offset", inode)))
    }
  }

  /// Synchronously read a raw inode from disk
  fn read_raw_inode<R: ?Sized>(&self, reader: &mut R, inode: u64) -> Result<raw_inode::EfsInode, SgidiskLibReadError>
    where R: Read + Seek
  {
    // Seek to start of inode data
    let offset = self.inode_start(inode)?;
    self.check_read_absolute(offset, raw_inode::EfsInode::SIZE as u64)?;
    reader.seek(SeekFrom::Start(offset))?;
    // Extract inode data
    raw_inode::EfsInode::read(reader)
  }

  /// Synchronously read an Inode from the filesystem
  pub fn read_inode<R: ?Sized>(&self, reader: &mut R, inode: u64) -> Result<Inode, SgidiskLibReadError>
    where R: Read + Seek {
    let raw = self.read_raw_inode(reader, inode)?;
    let mut inode = Inode::try_from(&raw)?;
    inode.normalize_extents(reader, self)?;
    Ok(inode)
  }

  /// Synchronously read / deserialize an Efs
  pub fn read<R: ?Sized>(reader: &mut R, sector_sz: u64, partition_start: u64) -> Result<Self, SgidiskLibReadError>
    where R: Read + Seek {
    // Read raw superblock
    reader.seek(SeekFrom::Start(partition_start))?;
    let raw = raw_sb::EfsSuperblock::read(reader)?;
    // Convert to Efs
    let mut efs = Efs::try_from((&raw, sector_sz, ))?;
    efs.partition_start = partition_start;
    Ok(efs)
  }

  /// Absolute offset to block in filesystem
  pub(crate) fn block_absolute(&self, block: u64) -> u64 {
    self.partition_start + block * EFS_BLOCK_SZ as u64
  }

  /// Synchronously seek to the numbered Basic Block in the filesystem
  pub(crate) fn seek_block<R: ?Sized>(&self, reader: &mut R, block: u64) -> Result<(), SgidiskLibReadError>
    where R: Seek {
    let offset = self.block_absolute(block);
    if offset > self.partition_start + self.size {
      return Err(SgidiskLibReadError::Bounds(format!("Requested block {} is beyond end of filesystem ({} bytes)", block, self.size)));
    }

    reader.seek(SeekFrom::Start(offset))?;
    Ok(())
  }
}

impl Inode {
  /// Iterator of block contents of Inode
  pub fn iter(&self) -> InodeBlockIter {
    InodeBlockIter {
      inode: self,
      extent: 0,
      block: 0,
    }
  }

  /// Normalize extents by expanding indirect extents (if applicable) and sorting them by
  /// position into file. Check that the values provided in the extents make sense.
  fn normalize_extents<R: ?Sized>(&mut self, reader: &mut R, efs: &Efs) -> Result<(), SgidiskLibReadError>
    where R: Read + Seek {
    self.expand_extents(reader, efs)?;
    self.sort_extents();
    self.check_extents()?;
    Ok(())
  }

  /// Check that the offset listed in each extent lines up with the cumulative
  /// lengths specified in previous extents
  fn check_extents(&self) -> Result<(), SgidiskLibReadError> {
    self.extents.iter()
      .try_fold(0 as u64, |offset, ext| {
        if offset == ext.ex_offset as u64 {
          Ok(offset + ext.ex_length as u64)
        } else {
          Err(SgidiskLibReadError::Value(format!("Next extent does not start ({}) where the previous one left off ({})", ext.ex_offset, offset)))
        }
      })?;
    Ok(())
  }

  /// If there are more extents than can fit in the inode (i.e. indirect extents),
  /// follow the references and replace all current extents with a list of the
  /// indirect extents.
  ///
  /// If there are few enough extents to fit in one block (i.e. direct extents),
  /// the current list of extents is left untouched.
  fn expand_extents<R: ?Sized>(&mut self, reader: &mut R, efs: &Efs) -> Result<(), SgidiskLibReadError>
    where R: Read + Seek {
    // If direct extents, nothing to expand
    if self.num_extents <= raw_inode::EfsInode::EFS_DIRECTEXTENTS {
      return Ok(());
    }

    let mut extents = Vec::with_capacity(self.num_extents);
    let mut indirect_remaining = self.num_extents;

    // For each direct extent
    for extent in &self.extents {
      // Find bounds of extent
      let from = efs.block_absolute(extent.ex_bn as u64);
      let sz = extent.ex_length as u64 * EFS_BLOCK_SZ as u64;
      efs.check_read_absolute(from, sz)?;
      // Seek to start of extent
      reader.seek(SeekFrom::Start(from))?;
      // For each block...
      for _block in 0..extent.ex_length {
        // Read block
        let block_read_sz = min(EFS_BLOCK_SZ, indirect_remaining * raw_inode::Extent::SIZE);
        let mut buf = vec![0; block_read_sz];
        reader.read_exact(&mut buf)?;
        // Parse extents
        let mut block_extents = raw_inode::Extent::parse_extents(&buf)?;
        indirect_remaining -= block_extents.len();
        extents.append(&mut block_extents);
      }
    }

    // Replace current list of extents
    self.extents = extents;
    Ok(())
  }

  /// Sort extents by position into file, ascending
  fn sort_extents(&mut self) {
    self.extents.sort_by_key(|e| e.ex_offset);
  }
}

impl TryFrom<(&raw_sb::EfsSuperblock, u64, )> for Efs {
  type Error = crate::SgidiskLibReadError;

  /// Convert from tuple of raw EfsSuperblock and sector size (in bytes)
  /// to public Efs struct
  fn try_from(value: (&raw_sb::EfsSuperblock, u64, )) -> Result<Self, Self::Error> {
    let (sb, sector_sz, ) = value;

    // Check and convert raw values, mostly oddly signed fields
    let size = match u64::try_from(sb.fs_size) {
      // Convert to bytes
      Ok(v) => v * sector_sz,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid FS size: {}", sb.fs_size)))
    };
    let cg_start = match u64::try_from(sb.fs_firstcg) {
      Ok(v) => v,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid CG start offset: {}", sb.fs_size)))
    };
    let cg_size = match u64::try_from(sb.fs_cgfsize) {
      Ok(v) => v,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid CG size: {}", sb.fs_size)))
    };
    // Check that the fs_cgisize is also a multiple of inode size
    let fs_cgisize_bytes = sb.fs_cgisize as i64 * EFS_BLOCK_SZ as i64;
    let cg_inodes = match (u64::try_from(fs_cgisize_bytes), fs_cgisize_bytes % raw_inode::EfsInode::SIZE as i64, ) {
      // Convert to number of inodes
      (Ok(v), 0, ) => v / raw_inode::EfsInode::SIZE as u64,
      _ => return Err(SgidiskLibReadError::Value(format!("Negative CG inode area size: {}", sb.fs_size)))
    };
    let cg_count = match u64::try_from(sb.fs_ncg) {
      Ok(v) => v,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid CG count: {}", sb.fs_size)))
    };

    Ok(Self {
      sector_sz,
      // Partition start must be set by caller, because we have no way of obbtaining that information
      partition_start: 0,
      size,
      cg_start,
      cg_size,
      cg_inodes,
      cg_count,
    })
  }
}

impl TryFrom<&raw_inode::EfsInode> for Inode {
  type Error = crate::SgidiskLibReadError;

  /// Convert from raw EfsInode to public Inode struct
  fn try_from(inode: &raw_inode::EfsInode) -> Result<Self, Self::Error> {
    use chrono::LocalResult;

    // Attempt to parse values
    let inode_type = match InodeType::try_from(inode.di_mode) {
      Ok(v) => v,
      Err(s) => return Err(SgidiskLibReadError::Value(s)),
    };
    let ctime = match Local.timestamp_opt(inode.di_ctime as i64, 0) {
      LocalResult::Single(t) => t,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid ctime: {}", inode.di_ctime)))
    };
    let mtime = match Local.timestamp_opt(inode.di_mtime as i64, 0) {
      LocalResult::Single(t) => t,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid mtime: {}", inode.di_mtime)))
    };
    let atime = match Local.timestamp_opt(inode.di_atime as i64, 0) {
      LocalResult::Single(t) => t,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid atime: {}", inode.di_atime)))
    };
    let size = match u64::try_from(inode.di_size) {
      Ok(n) => n,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid inode size: {}", inode.di_size)))
    };
    let unix_mode = inode.di_mode & raw_inode::EfsInode::INODE_MODE_MASK;

    // Parse extents
    let num_extents = match usize::try_from(inode.di_numextents) {
      Ok(n) => n,
      _ => return Err(SgidiskLibReadError::Value(format!("Invalid number of extents: {}", inode.di_numextents)))
    };
    if num_extents > raw_inode::Extent::MAX_EXTENTS {
      return Err(SgidiskLibReadError::Value(format!("Number of extents exceeds maximum: {}", inode.di_numextents)));
    }
    // Read a maximum of the number of listed extents, ignoring the rest of the payload
    let extent_sz = min(raw_inode::EfsInode::EXTENT_DATA_AREA_SZ, num_extents * raw_inode::Extent::SIZE);
    let extents: Vec<raw_inode::Extent> = raw_inode::Extent::parse_extents(&inode.data[0..extent_sz])?
      .into_iter()
      // Filter out any zero'ed extents
      .filter(|e| e.ex_length > 0)
      .collect();

    Ok(Inode {
      inode_type,
      unix_mode,
      owner_uid: inode.di_uid,
      owner_gid: inode.di_gid,
      size,
      ctime,
      mtime,
      atime,
      num_extents,
      extents,
    })
  }
}

impl TryFrom<u16> for InodeType {
  type Error = String;

  /// Convert inode type & permissions integer into InodeType enum
  fn try_from(bit_type: u16) -> Result<Self, Self::Error> {
    let itype_part = bit_type & raw_inode::EfsInode::INODE_TYPE_MASK;
    match itype_part {
      raw_inode::EfsInode::INODE_TYPE_FIFO => Ok(Self::Fifo),
      raw_inode::EfsInode::INODE_TYPE_FCHR => Ok(Self::CharacterSpecial),
      raw_inode::EfsInode::INODE_TYPE_FCHRLNK => Ok(Self::CharacterSpecialLink),
      raw_inode::EfsInode::INODE_TYPE_DIR => Ok(Self::Directory),
      raw_inode::EfsInode::INODE_TYPE_BLK => Ok(Self::BlockSpecial),
      raw_inode::EfsInode::INODE_TYPE_BLKLNK => Ok(Self::BlockSpecialLink),
      raw_inode::EfsInode::INODE_TYPE_REG => Ok(Self::RegularFile),
      raw_inode::EfsInode::INODE_TYPE_LNK => Ok(Self::SymbolicLink),
      raw_inode::EfsInode::INODE_TYPE_SOCK => Ok(Self::Socket),
      _ => Err(format!("Unknown inode type {}", itype_part))
    }
  }
}

impl From<InodeType> for u16 {
  /// Convert InodeType enum into bit field inode type
  fn from(inode_type: InodeType) -> Self {
    match inode_type {
      InodeType::Fifo => raw_inode::EfsInode::INODE_TYPE_FIFO,
      InodeType::CharacterSpecial => raw_inode::EfsInode::INODE_TYPE_FCHR,
      InodeType::CharacterSpecialLink => raw_inode::EfsInode::INODE_TYPE_FCHRLNK,
      InodeType::Directory => raw_inode::EfsInode::INODE_TYPE_DIR,
      InodeType::BlockSpecial => raw_inode::EfsInode::INODE_TYPE_BLK,
      InodeType::BlockSpecialLink => raw_inode::EfsInode::INODE_TYPE_BLKLNK,
      InodeType::RegularFile => raw_inode::EfsInode::INODE_TYPE_REG,
      InodeType::SymbolicLink => raw_inode::EfsInode::INODE_TYPE_LNK,
      InodeType::Socket => raw_inode::EfsInode::INODE_TYPE_SOCK
    }
  }
}

/// Iterator of blocks for an EFS Inode
pub struct InodeBlockIter<'a> {
  inode: &'a Inode,
  /// Extent within inode
  extent: usize,
  /// Block within extent
  block: usize,
}

impl<'a> Iterator for InodeBlockIter<'a> {
  type Item = u64;

  /// Get the number of the next block in this Inode
  fn next(&mut self) -> Option<Self::Item> {
    // If we are past our last extent, then there is nothing more to offer
    if self.extent >= self.inode.extents.len() {
      return None;
    }

    // Find extent and index current block offset over its base
    let extent = &self.inode.extents[self.extent];
    let block_num = extent.ex_bn as u64 + self.block as u64;

    // Wrap over to next extent if we've exceeded the number of blocks in this one
    self.block += 1;
    if self.block >= extent.ex_length as usize {
      self.extent += 1;
      self.block = 0;
    }

    Some(block_num)
  }
}

impl<'a> IntoIterator for &'a Inode {
  type Item = u64;
  type IntoIter = InodeBlockIter<'a>;

  fn into_iter(self) -> Self::IntoIter {
    self.iter()
  }
}