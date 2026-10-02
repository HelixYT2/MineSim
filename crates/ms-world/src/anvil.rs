//! Reading blocks out of a vanilla world's Anvil region files.
//!
//! Read-path only: enough to resolve the block state at a coordinate so collision can gather the
//! shapes around an entity. Parsed chunks are cached, since collision queries many blocks per
//! tick and re-decoding a region per block would be unusably slow, and so are the text-to-state-id
//! conversions. Chunk decoding is delegated to `fastanvil`/`fastnbt`.

use fastanvil::{Chunk, CurrentJavaChunk, Region};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A world backed by on-disk Anvil regions. Used to validate the kernel against real saves; the
/// chunk cache keeps repeated per-block queries cheap.
pub struct AnvilWorld {
    region_dir: PathBuf,
    chunks: Mutex<HashMap<(i32, i32), Option<CurrentJavaChunk>>>,
    states: Mutex<HashMap<String, u32>>,
}

impl AnvilWorld {
    pub fn new(region_dir: impl AsRef<Path>) -> Self {
        Self {
            region_dir: region_dir.as_ref().to_path_buf(),
            chunks: Mutex::new(HashMap::new()),
            states: Mutex::new(HashMap::new()),
        }
    }

    /// The block state id at a coordinate; air if the chunk/region is absent, the position is
    /// outside the build range, or the block is unknown to this version's registry.
    pub fn block_state(&self, x: i32, y: i32, z: i32) -> u32 {
        let Some(enc) = self.block_encoded(x, y, z) else {
            return ms_data::AIR;
        };
        if let Some(&s) = self.states.lock().unwrap().get(&enc) {
            return s;
        }
        let s = ms_data::parse_state(&enc).unwrap_or(ms_data::AIR);
        self.states.lock().unwrap().insert(enc, s);
        s
    }

    /// The block's `encoded_description` at a coordinate ("name|prop=val,..."), or `None` if the
    /// chunk/region is absent or the position is outside the build range.
    pub fn block_encoded(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let chunk_x = x.div_euclid(16);
        let chunk_z = z.div_euclid(16);
        if !self
            .chunks
            .lock()
            .unwrap()
            .contains_key(&(chunk_x, chunk_z))
        {
            let loaded = self.load_chunk(chunk_x, chunk_z);
            self.chunks
                .lock()
                .unwrap()
                .insert((chunk_x, chunk_z), loaded);
        }
        let cache = self.chunks.lock().unwrap();
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
