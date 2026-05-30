//! Reading blocks out of a vanilla world's Anvil region files.
//!
//! Read-path only: enough to resolve the block at a coordinate so collision can gather the
//! shapes around an entity. Parsed chunks are cached, since collision queries many blocks per
//! tick and re-decoding a region per block would be unusably slow. Chunk decoding is delegated
//! to `fastanvil`/`fastnbt`.

use fastanvil::{Chunk, CurrentJavaChunk, Region};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// A world backed by on-disk Anvil regions. Used to validate the kernel against real saves; the
/// chunk cache keeps repeated per-block queries cheap.
pub struct AnvilWorld {
    region_dir: PathBuf,
    chunks: RefCell<HashMap<(i32, i32), Option<CurrentJavaChunk>>>,
}

impl AnvilWorld {
    pub fn new(region_dir: impl AsRef<Path>) -> Self {
        Self {
            region_dir: region_dir.as_ref().to_path_buf(),
            chunks: RefCell::new(HashMap::new()),
        }
    }

    /// The block's `encoded_description` at a coordinate ("name|prop=val,..."), or `None` if the
    /// chunk/region is absent or the position is outside the build range.
    pub fn block_encoded(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let chunk_x = x.div_euclid(16);
        let chunk_z = z.div_euclid(16);
        if !self.chunks.borrow().contains_key(&(chunk_x, chunk_z)) {
            let loaded = self.load_chunk(chunk_x, chunk_z);
            self.chunks.borrow_mut().insert((chunk_x, chunk_z), loaded);
        }
        let cache = self.chunks.borrow();
        let chunk = cache.get(&(chunk_x, chunk_z))?.as_ref()?;
        let block = chunk.block(
            x.rem_euclid(16) as usize,
            y as isize,
            z.rem_euclid(16) as usize,
        )?;
        Some(block.encoded_description().to_string())
    }

    fn load_chunk(&self, chunk_x: i32, chunk_z: i32) -> Option<CurrentJavaChunk> {
        let region_x = chunk_x.div_euclid(32);
        let region_z = chunk_z.div_euclid(32);
        let path = self.region_dir.join(format!("r.{region_x}.{region_z}.mca"));
        let mut region = Region::from_stream(File::open(path).ok()?).ok()?;
        let data = region
            .read_chunk(
                chunk_x.rem_euclid(32) as usize,
                chunk_z.rem_euclid(32) as usize,
            )
            .ok()??;
        fastnbt::from_bytes(&data).ok()
    }
}
