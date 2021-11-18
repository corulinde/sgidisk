use std::collections::VecDeque;
use std::fs::File;

use thiserror::Error;

pub mod volhdr;
pub mod efs;

/// SGI Disk Library related errors
#[derive(Debug, Error)]
pub enum SgidiskLibReadError {
  #[error("Couldn't unpack binary data")]
  Unpack(#[from] deku::DekuError),
  #[error("I/O error")]
  Io(#[from] std::io::Error),
  #[error("Value error")]
  Value(String),
  #[error("File system points to something out of listed bounds")]
  Bounds(String),
}

pub fn fmt_inode(inode: &efs::Inode) -> String {
  format!("{:#?} {}:{} {} {:#?}",
          inode.inode_type,
          inode.owner_uid, inode.owner_gid,
          inode.size,
          inode.mtime)
}

pub fn bogus() {
  let fname = "/Users/elf/Downloads/IRIX 6.5.27 Installation Tools and Overlays (1 of 3).iso";
  let mut file = File::open(fname).unwrap();

  let vh = volhdr::SgidiskVolume::read(&mut file).unwrap();
  // println!("VOLHDR: {:#?}", &vh);

  // Find partition 7
  let p7 = &vh.partitions[7];
  let p7_start = p7.block_start * efs::EFS_BLOCK_SZ as u64;

  let efs = efs::Efs::read(&mut file, vh.sector_sz, p7_start).unwrap();
  // println!("SUPERBLOCK: {:#?}", &efs);

  let mut dir_deque: VecDeque<(u64, String)> = VecDeque::new();
  dir_deque.push_back((2, "".to_string()));

  while let Some((dir_inode, dir_name, )) = dir_deque.pop_front() {
    let dir_result = efs::dir::Directory::read_dir(&mut file, &efs, dir_inode);
    if dir_result.is_err() {
      println!("Problem on inode {} ({}): {:#?}", dir_inode, &dir_name, &dir_result);
    }
    let dir = dir_result.unwrap();
    for (entry_name, (entry_inode_id, entry_inode, )) in &dir.entries {
      if entry_inode.inode_type == efs::InodeType::Directory &&
        entry_name != "." &&
        entry_name != ".." {
        dir_deque.push_back((*entry_inode_id, format!("{}/{}", &dir_name, entry_name), ));
      }
      println!("{} {}/{}", fmt_inode(entry_inode), &dir_name, entry_name);
    }
  }
}

#[cfg(test)]
mod tests {
  #[test]
  fn it_works() {
    let result = 2 + 2;
    assert_eq!(result, 4);
  }

  #[test]
  fn bogus() {
    crate::bogus()
  }
}
