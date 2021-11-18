use std::collections::BTreeMap;
use std::io::{Read, Seek};

use crate::SgidiskLibReadError;

use super::{Inode, InodeType};
use super::raw_dir::DirectoryBlock;

/// Represents an EFS directory and its contents
#[derive(Debug)]
pub struct Directory {
  /// Inode of this directory
  pub directory_inode: Inode,
  /// Entries under this directory as (Inode ID, Inode) tuple
  pub entries: BTreeMap<String, (u64, Inode)>,
}

impl Directory {
  /// Inode number of root directory
  pub const ROOT_DIRECTORY_INODE: u64 = 2;
}

impl Directory {
  /// Synchronously read a directory listing from a numbered inode in an Efs.
  /// The root directory always starts at inode 2.
  pub fn read_dir<R: ?Sized>(reader: &mut R, efs: &super::Efs, inode: u64) -> Result<Directory, SgidiskLibReadError>
    where R: Read + Seek {
    // Read inode and check for directory
    let directory_inode = efs.read_inode(reader, inode)?;
    if directory_inode.inode_type != InodeType::Directory {
      return Err(SgidiskLibReadError::Value(format!("Inode {} is not a directory (is {:#?})", inode, directory_inode.inode_type)));
    }

    // Process each block in the inode as a DirectoryBlock
    let mut entries = BTreeMap::new();
    for block in &directory_inode {
      // Seek to block and read DirectoryBlock
      efs.check_read_block(block, DirectoryBlock::SIZE as u64)?;
      efs.seek_block(reader, block)?;
      let dir_block = DirectoryBlock::read(reader)?;

      // Fetch inode for each directory entry
      let block_entries = dir_block.dir_entries()?;
      for block_entry in &block_entries {
        let entry_name = match String::from_utf8(block_entry.d_name.clone()) {
          Ok(s) => s,
          _ => return Err(SgidiskLibReadError::Value(format!("Directory entry (inode {} block {}) name failed UTF8 conversion: {:#?}", inode, block, &block_entry)))
        };
        let entry_inode_id = block_entry.inode as u64;
        let entry_inode = efs.read_inode(reader, entry_inode_id)?;
        entries.insert(entry_name, (entry_inode_id, entry_inode, ));
      }
    }
    Ok(Directory {
      directory_inode,
      entries,
    })
  }
}