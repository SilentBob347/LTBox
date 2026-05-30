//! `/persist` fingerprint sync.
//!
//! Unlike the fixed-width country code, the fingerprint string changes length,
//! so the ext4 inode `i_size` is edited in addition to the file's data block.
//! Only the common Qualcomm `persist` layout is handled — an extent-mapped
//! inode, `metadata_csum` disabled, and `fingerprint.txt` in the filesystem
//! root. Anything else returns an error so the caller can warn and skip rather
//! than risk corrupting `persist`.

use fs_err as fs;
use std::path::Path;

use ltbox_core::{LtboxError, Result};
use tracing::info;

const SUPERBLOCK_OFFSET: usize = 1024;
const EXT4_MAGIC: u16 = 0xEF53;
const ROOT_INODE: u32 = 2;
const FINGERPRINT_FILENAME: &[u8] = b"fingerprint.txt";

// s_feature_incompat bits
const INCOMPAT_FILETYPE: u32 = 0x0002;
const INCOMPAT_EXTENTS: u32 = 0x0040;
const INCOMPAT_64BIT: u32 = 0x0080;
// s_feature_ro_compat bits
const RO_COMPAT_METADATA_CSUM: u32 = 0x0400;

// inode i_flags bit
const INODE_FLAG_EXTENTS: u32 = 0x0008_0000;
const EXTENT_HEADER_MAGIC: u16 = 0xF30A;
// ee_len values above this mark an uninitialized extent; real length is
// `ee_len - INIT_EXTENT_MAX`.
const INIT_EXTENT_MAX: usize = 32768;

fn err(msg: impl Into<String>) -> LtboxError {
    LtboxError::Patch(format!("persist fingerprint: {}", msg.into()))
}

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    let end = o.checked_add(2).ok_or_else(|| err("offset overflow"))?;
    d.get(o..end)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| err("read past end (u16)"))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    let end = o.checked_add(4).ok_or_else(|| err("offset overflow"))?;
    d.get(o..end)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| err("read past end (u32)"))
}

/// Checked multiply for image-controlled block/offset arithmetic — a corrupt
/// ext4 image must yield an error, never a debug panic or a wrapped (and
/// wrongly in-range) offset.
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(|| err("offset overflow"))
}

/// Checked add, same rationale as [`mul`].
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| err("offset overflow"))
}

/// Byte location of `fingerprint.txt`'s content + inode inside an ext4 image.
#[derive(Debug, Clone, Copy)]
struct FingerprintLoc {
    /// Byte offset of the file content (its first data block).
    data_offset: usize,
    /// Bytes available for content (allocated extent length × block size).
    capacity: usize,
    /// Byte offset of the inode (for the `i_size` edit).
    inode_offset: usize,
    /// Current `i_size`.
    current_size: usize,
}

/// Parsed ext4 geometry needed to walk inodes.
struct Ext4 {
    block_size: usize,
    inodes_per_group: u32,
    inode_size: usize,
    first_data_block: u32,
    desc_size: usize,
}

impl Ext4 {
    fn parse(data: &[u8]) -> Result<Self> {
        let sb = SUPERBLOCK_OFFSET;
        if u16_at(data, sb + 0x38)? != EXT4_MAGIC {
            return Err(err("not an ext4 image (bad magic)"));
        }
        let log_bs = u32_at(data, sb + 0x18)?;
        if log_bs > 6 {
            return Err(err("implausible block size"));
        }
        let block_size = 1024usize << log_bs;
        let inodes_per_group = u32_at(data, sb + 0x28)?;
        if inodes_per_group == 0 {
            return Err(err("zero inodes_per_group"));
        }
        let inode_size = u16_at(data, sb + 0x58)? as usize;
        if inode_size < 128 {
            return Err(err("inode_size too small"));
        }
        let first_data_block = u32_at(data, sb + 0x14)?;
        let feat_incompat = u32_at(data, sb + 0x60)?;
        let feat_ro = u32_at(data, sb + 0x64)?;

        if feat_incompat & INCOMPAT_EXTENTS == 0 {
            return Err(err("filesystem without extents not supported"));
        }
        if feat_incompat & INCOMPAT_FILETYPE == 0 {
            return Err(err("directory without file_type not supported"));
        }
        if feat_ro & RO_COMPAT_METADATA_CSUM != 0 {
            // Editing i_size would require recomputing the inode crc32c;
            // refuse rather than write an image the kernel rejects.
            return Err(err("metadata_csum enabled — refusing to edit i_size"));
        }

        let desc_size = if feat_incompat & INCOMPAT_64BIT != 0 {
            let s = u16_at(data, sb + 0xFE)? as usize;
            if s < 32 {
                return Err(err("bad desc_size"));
            }
            s
        } else {
            32
        };

        Ok(Self {
            block_size,
            inodes_per_group,
            inode_size,
            first_data_block,
            desc_size,
        })
    }

    /// Byte offset of inode `ino`'s on-disk record.
    fn inode_offset(&self, data: &[u8], ino: u32) -> Result<usize> {
        if ino == 0 {
            return Err(err("inode 0 is invalid"));
        }
        let group = (ino - 1) / self.inodes_per_group;
        let index = ((ino - 1) % self.inodes_per_group) as usize;
        // All factors below are image-controlled; keep the math checked so a
        // corrupt descriptor can't wrap to a bogus in-range offset.
        let gd = add(
            mul(self.first_data_block as usize + 1, self.block_size)?,
            mul(group as usize, self.desc_size)?,
        )?;
        let table_lo = u32_at(data, add(gd, 0x08)?)? as u64;
        let table_hi = if self.desc_size >= 64 {
            u32_at(data, add(gd, 0x28)?)? as u64
        } else {
            0
        };
        let table = ((table_hi << 32) | table_lo) as usize;
        let off = add(mul(table, self.block_size)?, mul(index, self.inode_size)?)?;
        if add(off, self.inode_size)? > data.len() {
            return Err(err("inode table entry out of range"));
        }
        Ok(off)
    }
}

/// Read the first (depth-0) extent at extent-header offset `eh`, returning
/// `(physical_start_block, block_count)`.
fn first_extent(data: &[u8], eh: usize) -> Result<(usize, usize)> {
    if u16_at(data, eh)? != EXTENT_HEADER_MAGIC {
        return Err(err("bad extent header magic"));
    }
    if u16_at(data, eh + 6)? != 0 {
        return Err(err("non-leaf extent tree not supported"));
    }
    if u16_at(data, eh + 2)? == 0 {
        return Err(err("file has no extents"));
    }
    let ee = eh + 12;
    let mut len = u16_at(data, ee + 4)? as usize;
    if len > INIT_EXTENT_MAX {
        len -= INIT_EXTENT_MAX;
    }
    if len == 0 {
        return Err(err("zero-length extent"));
    }
    let lo = u32_at(data, ee + 8)? as u64;
    let hi = u16_at(data, ee + 6)? as u64;
    let start = ((hi << 32) | lo) as usize;
    Ok((start, len))
}

/// Scan a directory data block for `FINGERPRINT_FILENAME`, returning its inode.
fn scan_dir_block(data: &[u8], block_off: usize, block_size: usize) -> Result<Option<u32>> {
    let end = block_off
        .checked_add(block_size)
        .filter(|&e| e <= data.len())
        .ok_or_else(|| err("directory block out of range"))?;
    let mut o = block_off;
    while o + 8 <= end {
        let ino = u32_at(data, o)?;
        let rec_len = u16_at(data, o + 4)? as usize;
        let name_len = data[o + 6] as usize;
        if rec_len < 8 {
            break; // malformed; stop scanning this block
        }
        if ino != 0
            && name_len == FINGERPRINT_FILENAME.len()
            && o + 8 + name_len <= end
            && &data[o + 8..o + 8 + name_len] == FINGERPRINT_FILENAME
        {
            return Ok(Some(ino));
        }
        o += rec_len;
    }
    Ok(None)
}

/// Walk the (extent-mapped, depth-0) root directory for `fingerprint.txt`.
fn find_fingerprint_inode(data: &[u8], fs: &Ext4) -> Result<Option<u32>> {
    let root = fs.inode_offset(data, ROOT_INODE)?;
    if u32_at(data, root + 0x20)? & INODE_FLAG_EXTENTS == 0 {
        return Err(err("root directory is not extent-mapped"));
    }
    let eh = root + 0x28;
    if u16_at(data, eh)? != EXTENT_HEADER_MAGIC {
        return Err(err("bad root extent header"));
    }
    if u16_at(data, eh + 6)? != 0 {
        return Err(err("non-leaf root extent tree not supported"));
    }
    let entries = u16_at(data, eh + 2)? as usize;
    for k in 0..entries {
        let ee = eh + 12 + k * 12;
        let mut len = u16_at(data, ee + 4)? as usize;
        if len > INIT_EXTENT_MAX {
            len -= INIT_EXTENT_MAX;
        }
        let lo = u32_at(data, ee + 8)? as u64;
        let hi = u16_at(data, ee + 6)? as u64;
        let start = ((hi << 32) | lo) as usize;
        for b in 0..len {
            let block_off = mul(add(start, b)?, fs.block_size)?;
            if let Some(ino) = scan_dir_block(data, block_off, fs.block_size)? {
                return Ok(Some(ino));
            }
        }
    }
    Ok(None)
}

fn locate_fingerprint(data: &[u8]) -> Result<FingerprintLoc> {
    let fs = Ext4::parse(data)?;
    let ino = find_fingerprint_inode(data, &fs)?
        .ok_or_else(|| err("fingerprint.txt not found in filesystem root"))?;
    let inode_offset = fs.inode_offset(data, ino)?;
    if u32_at(data, inode_offset + 0x20)? & INODE_FLAG_EXTENTS == 0 {
        return Err(err("fingerprint.txt is not extent-mapped"));
    }
    let current_size = u32_at(data, inode_offset + 0x04)? as usize
        | ((u32_at(data, inode_offset + 0x6C)? as usize) << 32);
    let (block, len) = first_extent(data, inode_offset + 0x28)?;
    let data_offset = mul(block, fs.block_size)?;
    let capacity = mul(len, fs.block_size)?;
    if add(data_offset, capacity)? > data.len() {
        return Err(err("data block out of range"));
    }
    Ok(FingerprintLoc {
        data_offset,
        capacity,
        inode_offset,
        current_size,
    })
}

/// Overwrite `/persist`'s `fingerprint.txt` content with `new_fp`, editing the
/// inode `i_size` to match. Returns `Ok(true)` if the content changed,
/// `Ok(false)` if it already matched. Errors describe an unsupported layout so
/// the caller can warn and skip.
pub fn patch_persist_fingerprint_bytes(data: &mut [u8], new_fp: &str) -> Result<bool> {
    let loc = locate_fingerprint(data)?;
    let new = new_fp.as_bytes();
    if new.len() > loc.capacity {
        return Err(err(format!(
            "fingerprint ({} bytes) exceeds file capacity ({} bytes)",
            new.len(),
            loc.capacity
        )));
    }
    let cur_len = loc.current_size.min(loc.capacity);
    if data[loc.data_offset..loc.data_offset + cur_len] == *new {
        return Ok(false);
    }
    // Zero the previously-used region (clears any leftover when the new string
    // is shorter), then write the new content. Beyond `used` the block is
    // already zero-padded.
    let used = loc.current_size.max(new.len()).min(loc.capacity);
    data[loc.data_offset..loc.data_offset + used].fill(0);
    data[loc.data_offset..loc.data_offset + new.len()].copy_from_slice(new);
    // i_size_lo @ +0x04, i_size_high @ +0x6C. Fingerprints are well under 4 GiB.
    let nlen = new.len() as u32;
    data[loc.inode_offset + 0x04..loc.inode_offset + 0x08].copy_from_slice(&nlen.to_le_bytes());
    data[loc.inode_offset + 0x6C..loc.inode_offset + 0x70].copy_from_slice(&0u32.to_le_bytes());
    Ok(true)
}

/// File wrapper around [`patch_persist_fingerprint_bytes`]. Reads `img_path`,
/// patches in memory, and rewrites only when the content changed.
pub fn patch_persist_fingerprint(img_path: &Path, new_fp: &str) -> Result<bool> {
    let mut data =
        fs::read(img_path).map_err(|e| err(format!("read {}: {e}", img_path.display())))?;
    let changed = patch_persist_fingerprint_bytes(&mut data, new_fp)?;
    if changed {
        fs::write(img_path, &data)
            .map_err(|e| err(format!("write {}: {e}", img_path.display())))?;
        info!(
            "persist fingerprint.txt -> {new_fp:?} ({})",
            img_path.display()
        );
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a tiny but structurally valid ext4 image (1 KiB blocks) holding a
    // single root-level `fingerprint.txt` so the parser/patcher can be tested
    // without a real 32 MiB persist dump.
    struct Layout {
        inode_table_block: usize,
        root_dir_block: usize,
        fp_data_block: usize,
        fp_inode: u32,
    }

    fn put_u16(d: &mut [u8], o: usize, v: u16) {
        d[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(d: &mut [u8], o: usize, v: u32) {
        d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn write_extent_header(d: &mut [u8], at: usize, entries: u16) {
        put_u16(d, at, EXTENT_HEADER_MAGIC);
        put_u16(d, at + 2, entries); // eh_entries
        put_u16(d, at + 4, 4); // eh_max
        put_u16(d, at + 6, 0); // eh_depth
        put_u32(d, at + 8, 0); // eh_generation
    }
    fn write_extent_entry(d: &mut [u8], at: usize, file_block: u32, len: u16, phys: u32) {
        put_u32(d, at, file_block);
        put_u16(d, at + 4, len);
        put_u16(d, at + 6, 0); // start_hi
        put_u32(d, at + 8, phys);
    }

    fn build_image(old_fp: &[u8], metadata_csum: bool) -> (Vec<u8>, Layout) {
        let block_size = 1024usize;
        let inode_size = 128usize;
        let lay = Layout {
            inode_table_block: 4,
            root_dir_block: 8,
            fp_data_block: 9,
            fp_inode: 3,
        };
        let mut d = vec![0u8; 16 * block_size];

        // Superblock @ 1024.
        let sb = SUPERBLOCK_OFFSET;
        put_u32(&mut d, sb + 0x14, 1); // first_data_block (1 KiB blocks)
        put_u32(&mut d, sb + 0x18, 0); // log_block_size -> 1024
        put_u32(&mut d, sb + 0x28, 16); // inodes_per_group
        put_u16(&mut d, sb + 0x38, EXT4_MAGIC);
        put_u16(&mut d, sb + 0x58, inode_size as u16);
        put_u32(&mut d, sb + 0x60, INCOMPAT_EXTENTS | INCOMPAT_FILETYPE);
        put_u32(
            &mut d,
            sb + 0x64,
            if metadata_csum {
                RO_COMPAT_METADATA_CSUM
            } else {
                0
            },
        );

        // Group descriptor 0 @ block 2.
        let gd = 2 * block_size;
        put_u32(&mut d, gd + 0x08, lay.inode_table_block as u32);

        // Root inode (2): directory, extent -> root_dir_block.
        let it = lay.inode_table_block * block_size;
        let root = it + inode_size; // inode 2 -> table index 1
        put_u16(&mut d, root, 0x41ED); // i_mode (offset 0x00): dir
        put_u32(&mut d, root + 0x04, block_size as u32); // i_size
        put_u32(&mut d, root + 0x20, INODE_FLAG_EXTENTS);
        write_extent_header(&mut d, root + 0x28, 1);
        write_extent_entry(&mut d, root + 0x28 + 12, 0, 1, lay.root_dir_block as u32);

        // fingerprint.txt inode (3): regular, extent -> fp_data_block.
        let fino = it + 2 * inode_size;
        put_u16(&mut d, fino, 0x81A4); // i_mode (offset 0x00): regular
        put_u32(&mut d, fino + 0x04, old_fp.len() as u32); // i_size
        put_u32(&mut d, fino + 0x20, INODE_FLAG_EXTENTS);
        write_extent_header(&mut d, fino + 0x28, 1);
        write_extent_entry(&mut d, fino + 0x28 + 12, 0, 1, lay.fp_data_block as u32);

        // Root directory block: '.', '..', 'fingerprint.txt'.
        let dir = lay.root_dir_block * block_size;
        put_u32(&mut d, dir, 2);
        put_u16(&mut d, dir + 4, 12);
        d[dir + 6] = 1;
        d[dir + 7] = 2;
        d[dir + 8] = b'.';
        put_u32(&mut d, dir + 12, 2);
        put_u16(&mut d, dir + 16, 12);
        d[dir + 18] = 2;
        d[dir + 19] = 2;
        d[dir + 20] = b'.';
        d[dir + 21] = b'.';
        let e = dir + 24;
        put_u32(&mut d, e, lay.fp_inode);
        put_u16(&mut d, e + 4, (block_size - 24) as u16); // rec_len to block end
        d[e + 6] = FINGERPRINT_FILENAME.len() as u8;
        d[e + 7] = 1; // regular file
        d[e + 8..e + 8 + FINGERPRINT_FILENAME.len()].copy_from_slice(FINGERPRINT_FILENAME);

        // fingerprint.txt content.
        let fp = lay.fp_data_block * block_size;
        d[fp..fp + old_fp.len()].copy_from_slice(old_fp);

        (d, lay)
    }

    fn inode_size_field(d: &[u8], lay: &Layout) -> u32 {
        let fino = lay.inode_table_block * 1024 + 2 * 128;
        u32::from_le_bytes([d[fino + 4], d[fino + 5], d[fino + 6], d[fino + 7]])
    }

    #[test]
    fn syncs_longer_fingerprint_and_updates_i_size() {
        let old = b"Lenovo/TB323FU/TB323FU:16/BQ2A.250831.001/10.084.260421W:user/release-keys";
        let new = "Lenovo/TB323FU/TB323FU:16/BQ2A.250831.001-BP2A.250605.031.A3/10.084.260421W:user/release-keys";
        let (mut img, lay) = build_image(old, false);

        let changed = patch_persist_fingerprint_bytes(&mut img, new).unwrap();
        assert!(changed);

        let fp = lay.fp_data_block * 1024;
        assert_eq!(&img[fp..fp + new.len()], new.as_bytes());
        // byte after the new content stays null-terminated
        assert_eq!(img[fp + new.len()], 0);
        assert_eq!(inode_size_field(&img, &lay), new.len() as u32);

        // idempotent second run
        assert!(!patch_persist_fingerprint_bytes(&mut img, new).unwrap());
    }

    #[test]
    fn syncs_shorter_fingerprint_and_zeroes_tail() {
        let old = b"Lenovo/TB323FU/TB323FU:16/BQ2A.250831.001-BP2A.250605.031.A3/10.084.260421W:user/release-keys";
        let new = "Lenovo/TB320FC/TB320FC:16/x/y:user/release-keys";
        let (mut img, lay) = build_image(old, false);

        assert!(patch_persist_fingerprint_bytes(&mut img, new).unwrap());
        let fp = lay.fp_data_block * 1024;
        assert_eq!(&img[fp..fp + new.len()], new.as_bytes());
        // old trailing bytes beyond the shorter new string are cleared
        assert!(img[fp + new.len()..fp + old.len()].iter().all(|&b| b == 0));
        assert_eq!(inode_size_field(&img, &lay), new.len() as u32);
    }

    #[test]
    fn refuses_metadata_csum() {
        let (mut img, _) = build_image(b"Lenovo/x:user/release-keys", true);
        let e =
            patch_persist_fingerprint_bytes(&mut img, "Lenovo/y:user/release-keys").unwrap_err();
        assert!(format!("{e}").contains("metadata_csum"), "got: {e}");
    }

    #[test]
    fn refuses_non_ext4() {
        let mut img = vec![0u8; 8192];
        let e = patch_persist_fingerprint_bytes(&mut img, "x").unwrap_err();
        assert!(format!("{e}").contains("bad magic"), "got: {e}");
    }

    #[test]
    fn refuses_fingerprint_too_large() {
        let (mut img, _) = build_image(b"Lenovo/x:user/release-keys", false);
        // capacity is one 1 KiB block; ask for more.
        let huge = "A".repeat(2000);
        let e = patch_persist_fingerprint_bytes(&mut img, &huge).unwrap_err();
        assert!(format!("{e}").contains("exceeds file capacity"), "got: {e}");
    }

    #[test]
    fn checked_arithmetic_rejects_overflow() {
        assert!(mul(usize::MAX, 2).is_err());
        assert!(add(usize::MAX, 1).is_err());
    }

    #[test]
    fn refuses_corrupt_inode_table_pointer() {
        // A group descriptor pointing far outside the image must produce a
        // graceful error, never a panic or a wrapped in-range offset.
        let (mut img, _) = build_image(b"Lenovo/x:user/release-keys", false);
        let gd = 2 * 1024; // group descriptor 0 @ block 2 (1 KiB blocks)
        put_u32(&mut img, gd + 0x08, 0x00FF_FFFF); // absurd inode-table block
        let e =
            patch_persist_fingerprint_bytes(&mut img, "Lenovo/y:user/release-keys").unwrap_err();
        assert!(
            format!("{e}").contains("out of range") || format!("{e}").contains("overflow"),
            "got: {e}"
        );
    }

    // Opt-in check against a real dump: set LTBOX_PERSIST_IMG to a copy of a
    // `persist.img`. Verifies parsing + a no-op/real patch round-trips.
    #[test]
    fn real_persist_image_when_available() {
        let Some(path) = std::env::var_os("LTBOX_PERSIST_IMG") else {
            return;
        };
        let mut data = std::fs::read(&path).unwrap();
        let loc = locate_fingerprint(&data).expect("locate fingerprint.txt");
        assert!(loc.current_size > 0 && loc.current_size <= loc.capacity);
        let new = "Lenovo/TEST/TEST:16/BUILD-ID/incremental:user/release-keys";
        assert!(patch_persist_fingerprint_bytes(&mut data, new).unwrap());
        let got = &data[loc.data_offset..loc.data_offset + new.len()];
        assert_eq!(got, new.as_bytes());
    }
}
