//! `net.minecraft.world.level.levelgen.RandomSupport` and the hashing it relies on.

/// `RandomSupport.GOLDEN_RATIO_64`.
pub const GOLDEN_RATIO_64: i64 = -7046029254386353131;
/// `RandomSupport.SILVER_RATIO_64`.
pub const SILVER_RATIO_64: i64 = 7640891576956012809;

/// `RandomSupport.Seed128bit`: the two halves of a Xoroshiro128++ state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Seed128bit {
    pub seed_lo: i64,
    pub seed_hi: i64,
}

impl Seed128bit {
    pub const fn new(seed_lo: i64, seed_hi: i64) -> Self {
        Self { seed_lo, seed_hi }
    }

    /// `Seed128bit.xor(long, long)`.
    pub const fn xor(self, lo: i64, hi: i64) -> Self {
        Self::new(self.seed_lo ^ lo, self.seed_hi ^ hi)
    }

    /// `Seed128bit.mixed()`: each half through [`mix_stafford13`].
    pub const fn mixed(self) -> Self {
        Self::new(mix_stafford13(self.seed_lo), mix_stafford13(self.seed_hi))
    }
}

/// `RandomSupport.mixStafford13`: Stafford's "Mix13" 64-bit finaliser.
pub const fn mix_stafford13(l: i64) -> i64 {
    let mut v = l as u64;
    v = (v ^ (v >> 30)).wrapping_mul(-4658895280553007687_i64 as u64);
    v = (v ^ (v >> 27)).wrapping_mul(-7723592293110705685_i64 as u64);
    (v ^ (v >> 31)) as i64
}

/// `RandomSupport.upgradeSeedTo128bitUnmixed`.
pub const fn upgrade_seed_to_128bit_unmixed(l: i64) -> Seed128bit {
    let m = l ^ SILVER_RATIO_64;
    let n = m.wrapping_add(GOLDEN_RATIO_64);
    Seed128bit::new(m, n)
}

/// `RandomSupport.upgradeSeedTo128bit`: how `RandomSource.create`-style 64-bit seeds become
/// Xoroshiro states.
pub const fn upgrade_seed_to_128bit(l: i64) -> Seed128bit {
    upgrade_seed_to_128bit_unmixed(l).mixed()
}

/// `RandomSupport.seedFromHashOf(String)`: the MD5 of the UTF-8 bytes, read as two big-endian
/// longs.
pub fn seed_from_hash_of(s: &str) -> Seed128bit {
    let digest = md5(s.as_bytes());
    let lo = i64::from_be_bytes(digest[0..8].try_into().unwrap());
    let hi = i64::from_be_bytes(digest[8..16].try_into().unwrap());
    Seed128bit::new(lo, hi)
}

/// `String.hashCode()`: `31 * h + c` over the UTF-16 code units, wrapping in 32 bits.
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    h
}

// ---------------------------------------------------------------------------------------------
// MD5 (RFC 1321), only for `seed_from_hash_of`
// ---------------------------------------------------------------------------------------------

const MD5_K: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

const MD5_SHIFT: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// The MD5 digest of `data`.
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());

    for block in message.chunks_exact(64) {
        let mut words = [0u32; 16];
        for (i, word) in words.iter_mut().enumerate() {
            *word = u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(MD5_K[i])
                .wrapping_add(words[g])
                .rotate_left(MD5_SHIFT[i]);
            let next_b = b.wrapping_add(rotated);
            a = d;
            d = c;
            c = b;
            b = next_b;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn md5_rfc1321_suite() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"a")), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(&md5(b"message digest")),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        assert_eq!(
            hex(&md5(b"abcdefghijklmnopqrstuvwxyz")),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
        assert_eq!(
            hex(&md5(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            )),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn java_string_hash_known_values() {
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("a"), 97);
        assert_eq!(java_string_hash("Hello, World!"), 1498789909);
    }

    #[test]
    fn mix_is_a_bijection_on_samples() {
        assert_eq!(mix_stafford13(0), 0);
        assert_ne!(mix_stafford13(1), mix_stafford13(2));
    }
}
