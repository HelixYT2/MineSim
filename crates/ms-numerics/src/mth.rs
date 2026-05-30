//! The sine/cosine tables the game uses for entity rotation.
//!
//! Rather than calling `Math.sin` per tick, the game indexes a 65536-entry table of
//! precomputed `(float) Math.sin(...)` values. Matching movement therefore means matching
//! that exact table, so it is generated from the JVM by the tool in `tools/refgen` and
//! embedded here verbatim.

const SIN_TABLE: &[u8] = include_bytes!("../data/mth_sin_table.bin");

const _: () = assert!(SIN_TABLE.len() == 65536 * 4);

fn lookup(index: usize) -> f32 {
    let i = index * 4;
    f32::from_le_bytes([
        SIN_TABLE[i],
        SIN_TABLE[i + 1],
        SIN_TABLE[i + 2],
        SIN_TABLE[i + 3],
    ])
}

pub fn sin(value: f32) -> f32 {
    lookup(((value * 10430.378_f32) as i32 & 0xffff) as usize)
}

pub fn cos(value: f32) -> f32 {
    lookup(((value * 10430.378_f32 + 16384.0_f32) as i32 & 0xffff) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_jvm_reference() {
        let csv = include_str!("../testdata/mth_reference.csv");
        for line in csv.lines().skip(1) {
            let mut cols = line.split(',');
            let x = f32::from_bits(cols.next().unwrap().parse().unwrap());
            let want_sin: u32 = cols.next().unwrap().parse().unwrap();
            let want_cos: u32 = cols.next().unwrap().parse().unwrap();
            assert_eq!(sin(x).to_bits(), want_sin, "sin({x})");
            assert_eq!(cos(x).to_bits(), want_cos, "cos({x})");
        }
    }
}
