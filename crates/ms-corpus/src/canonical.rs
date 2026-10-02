//! The oracle side of the state hash: the recorded corpus states serialized into the `contract-v1`
//! bytes (`docs/contract.md` §2).
//!
//! The simulator serializes a [`PlayerState`] with `ms_oracle::player::serialize_player`; the
//! recorded states are plain field maps, serialized here by walking `ms_oracle::player::LAYOUT`.
//! The two implementations are independent, and a test over every recorded tick checks that they
//! agree, which is what makes `H_sim(t) == H_oracle(t)` meaningful.

use crate::{apply_state, f64_bits, Fields};
use ms_kernel::attributes::Attribute;
use ms_kernel::PlayerState;
use ms_numerics::Vec3;
use ms_oracle::player::{pose_id_from_name, Kind, CONTRACT_VERSION, LAYOUT};
use ms_oracle::StateBuf;
use serde_json::Value;

fn int(f: &Fields, key: &str) -> Result<i64, String> {
    f.get(key)
        .ok_or_else(|| format!("missing field `{key}`"))?
        .as_i64()
        .ok_or_else(|| format!("field `{key}` is not an integer"))
}

/// The canonical bytes of one recorded (complete) state: the version byte, then every field of
/// `ms_oracle::player::LAYOUT` in order. Fails if the state lacks a field of the layout (`support`
/// is the one legitimately optional field: absent means no supporting block).
pub fn fields_bytes(f: &Fields) -> Result<Vec<u8>, String> {
    let mut b = StateBuf::new();
    b.push_u8(CONTRACT_VERSION);
    for &(name, kind) in LAYOUT {
        match kind {
            Kind::F64 => {
                b.push_u64(int(f, name)? as u64);
            }
            Kind::F32 => {
                b.push_u32(int(f, name)? as i32 as u32);
            }
            Kind::Bool => {
                b.push_bool(int(f, name)? == 1);
            }
            Kind::I32 => {
                let v = int(f, name)?;
                b.push_i32(i32::try_from(v).map_err(|_| format!("`{name}` out of i32 range"))?);
            }
            Kind::Pose => {
                let text = f
                    .get(name)
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("missing or non-string field `{name}`"))?;
                b.push_u8(pose_id_from_name(text).ok_or_else(|| format!("unknown pose `{text}`"))?);
            }
            Kind::Support => match f.get(name) {
                Some(v) => {
                    let packed = v.as_i64().ok_or("`support` is not an integer")?;
                    b.push_bool(true).push_i64(packed);
                }
                None => {
                    b.push_bool(false).push_i64(0);
                }
            },
            Kind::Effects => {
                let list = f
                    .get(name)
                    .and_then(Value::as_array)
                    .ok_or("missing or non-array field `effects`")?;
                let mut items = Vec::with_capacity(list.len());
                for e in list {
                    let id = e["id"].as_str().ok_or("effect without an id")?;
                    let amp = e["amp"].as_i64().ok_or("effect without an amplifier")?;
                    let dur = e["dur"].as_i64().ok_or("effect without a duration")?;
                    items.push((id, amp as i32, dur as i32));
                }
                items.sort_by(|a, c| a.0.as_bytes().cmp(c.0.as_bytes()));
                b.push_i32(items.len() as i32);
                for (id, amp, dur) in items {
                    b.push_str(id).push_i32(amp).push_i32(dur);
                }
            }
            Kind::Attrs => {
                let attrs = f
                    .get(name)
                    .and_then(Value::as_object)
                    .ok_or("missing or non-object field `attrs`")?;
                for a in Attribute::ALL {
                    let v = attrs
                        .get(a.name())
                        .ok_or_else(|| format!("missing attribute `{}`", a.name()))?;
                    b.push_u64(v.as_i64().ok_or("attribute is not an integer")? as u64);
                }
            }
        }
    }
    Ok(b.into_bytes())
}

/// `H(t)` of a recorded state: `xxh3_64(seed, fields_bytes)`.
pub fn fields_hash(f: &Fields) -> Result<u64, String> {
    Ok(ms_oracle::hash_bytes(&fields_bytes(f)?))
}

/// Write the recorded attribute values into `p` as base values. Only meaningful for a state with
/// no modifiers (a fresh [`PlayerState`]); the replay driver does *not* use it, because the
/// kernel owns the effect/sprint modifiers. It exists to build a [`PlayerState`] whose hash can
/// be compared with the recorded one.
pub fn apply_attrs(p: &mut PlayerState, f: &Fields) {
    if let Some(attrs) = f.get("attrs").and_then(Value::as_object) {
        for a in Attribute::ALL {
            if let Some(v) = attrs.get(a.name()) {
                p.attributes.set_base(a, f64_bits(v));
            }
        }
    }
}

/// A [`PlayerState`] equal to the recorded state `f` in every hashed field (a fresh state with
/// [`apply_state`] and [`apply_attrs`] applied).
pub fn state_from_fields(f: &Fields) -> PlayerState {
    let mut p = PlayerState::new(Vec3::ZERO, 0.0);
    apply_state(&mut p, f);
    apply_attrs(&mut p, f);
    if !f.contains_key("support") {
        p.supporting_block = None;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk_row0() -> Fields {
        let s = crate::Scenario::load("walk_basic").unwrap();
        s.rows[0].post.clone()
    }

    #[test]
    fn hash_of_bytes_matches_state_buf_hash() {
        let f = walk_row0();
        let bytes = fields_bytes(&f).unwrap();
        let mut direct = StateBuf::new();
        for &x in &bytes {
            direct.push_u8(x);
        }
        assert_eq!(fields_hash(&f).unwrap(), direct.hash());
        assert_eq!(bytes[0], CONTRACT_VERSION);
        assert_eq!(direct.bytes(), bytes.as_slice());
    }

    #[test]
    fn missing_and_malformed_fields_are_errors() {
        let mut f = walk_row0();
        f.remove("dx");
        assert!(fields_bytes(&f).unwrap_err().contains("dx"));
        let mut f = walk_row0();
        f.insert("pose".into(), Value::from("LEVITATING"));
        assert!(fields_bytes(&f).unwrap_err().contains("pose"));
        let mut f = walk_row0();
        f.remove("attrs");
        assert!(fields_bytes(&f).unwrap_err().contains("attrs"));
    }

    #[test]
    fn absent_support_serializes_as_no_block() {
        let mut f = walk_row0();
        f.remove("support");
        let a = fields_bytes(&f).unwrap();
        f.insert("support".into(), Value::from(0));
        let b = fields_bytes(&f).unwrap();
        assert_ne!(a, b, "a block at the origin is not 'no block'");
        assert_eq!(a.len(), b.len());
    }
}
