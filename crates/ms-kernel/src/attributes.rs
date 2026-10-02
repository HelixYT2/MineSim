//! Entity attributes (`AttributeInstance`): a base value plus modifiers grouped by operation,
//! combined exactly as the game does: add values, then base-multipliers, then each total-multiplier
//! in turn. Status effects and sprinting act on movement through modifiers here.
//!
//! # Why the modifier order matters
//!
//! `AttributeInstance.calculateValue` folds the modifiers of one operation one after another, and
//! floating-point multiplication is not associative: speed, slowness and sprinting are all
//! `ADD_MULTIPLIED_TOTAL`, so `((base * (1 + sprint)) * (1 + speed)) * (1 + slow)` and the same
//! product in another order can differ in the last bit. The game keeps the modifiers of each
//! operation in a fastutil `Object2ObjectOpenHashMap` keyed by the modifier's `Identifier` and folds
//! them in that map's `values()` order. That order is the open-addressing table order walked from
//! the highest slot to the lowest, so it depends on `Identifier.hashCode`, fastutil's
//! `HashCommon.mix`, the table capacity and its growth/shrink rules, and (for colliding keys) the
//! insertion history. `Table` (private) reproduces all of that; [`identifier_hash`] is the key hash.
//!
//! The per-operation tables hold indices into the instance's insertion-ordered modifier list
//! (the game's `modifierById`, an `Object2ObjectArrayMap`), which is also the order the server
//! writes modifiers into an attribute-update packet.

/// The attributes that influence the simulated state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Attribute {
    MovementSpeed,
    JumpStrength,
    Gravity,
    StepHeight,
    SafeFallDistance,
    FallDamageMultiplier,
    KnockbackResistance,
    WaterMovementEfficiency,
    MovementEfficiency,
    SneakingSpeed,
    MaxHealth,
    Armor,
    Scale,
    BurningTime,
}

impl Attribute {
    pub const ALL: [Attribute; 14] = [
        Attribute::MovementSpeed,
        Attribute::JumpStrength,
        Attribute::Gravity,
        Attribute::StepHeight,
        Attribute::SafeFallDistance,
        Attribute::FallDamageMultiplier,
        Attribute::KnockbackResistance,
        Attribute::WaterMovementEfficiency,
        Attribute::MovementEfficiency,
        Attribute::SneakingSpeed,
        Attribute::MaxHealth,
        Attribute::Armor,
        Attribute::Scale,
        Attribute::BurningTime,
    ];

    /// Position in [`Attribute::ALL`].
    fn index(self) -> usize {
        self as usize
    }

    /// The short name the oracle corpus uses (the registry path).
    pub fn name(self) -> &'static str {
        match self {
            Attribute::MovementSpeed => "movement_speed",
            Attribute::JumpStrength => "jump_strength",
            Attribute::Gravity => "gravity",
            Attribute::StepHeight => "step_height",
            Attribute::SafeFallDistance => "safe_fall_distance",
            Attribute::FallDamageMultiplier => "fall_damage_multiplier",
            Attribute::KnockbackResistance => "knockback_resistance",
            Attribute::WaterMovementEfficiency => "water_movement_efficiency",
            Attribute::MovementEfficiency => "movement_efficiency",
            Attribute::SneakingSpeed => "sneaking_speed",
            Attribute::MaxHealth => "max_health",
            Attribute::Armor => "armor",
            Attribute::Scale => "scale",
            Attribute::BurningTime => "burning_time",
        }
    }

    /// The player's base value for this attribute (`Player.createAttributes` over the registry
    /// defaults). Two of the Java sources are `float` literals widened to `double`
    /// (`MOVEMENT_SPEED` 0.1F, `JUMP_STRENGTH` 0.42F); the rest are `double` literals.
    pub fn player_base(self) -> f64 {
        match self {
            Attribute::MovementSpeed => f64::from(0.1_f32),
            Attribute::JumpStrength => f64::from(0.42_f32),
            Attribute::Gravity => 0.08,
            Attribute::StepHeight => 0.6,
            Attribute::SafeFallDistance => 3.0,
            Attribute::FallDamageMultiplier => 1.0,
            Attribute::KnockbackResistance => 0.0,
            Attribute::WaterMovementEfficiency => 0.0,
            Attribute::MovementEfficiency => 0.0,
            Attribute::SneakingSpeed => 0.3,
            Attribute::MaxHealth => 20.0,
            Attribute::Armor => 0.0,
            Attribute::Scale => 1.0,
            Attribute::BurningTime => 1.0,
        }
    }

    /// The `RangedAttribute` bounds `(min, max)`.
    pub fn range(self) -> (f64, f64) {
        match self {
            Attribute::MovementSpeed => (0.0, 1024.0),
            Attribute::JumpStrength => (0.0, 32.0),
            Attribute::Gravity => (-1.0, 1.0),
            Attribute::StepHeight => (0.0, 10.0),
            Attribute::SafeFallDistance => (-1024.0, 1024.0),
            Attribute::FallDamageMultiplier => (0.0, 100.0),
            Attribute::KnockbackResistance => (0.0, 1.0),
            Attribute::WaterMovementEfficiency => (0.0, 1.0),
            Attribute::MovementEfficiency => (0.0, 1.0),
            Attribute::SneakingSpeed => (0.0, 1.0),
            Attribute::MaxHealth => (1.0, 1024.0),
            Attribute::Armor => (0.0, 30.0),
            Attribute::Scale => (0.0625, 16.0),
            Attribute::BurningTime => (0.0, 1024.0),
        }
    }
}

/// `AttributeModifier.Operation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

impl Operation {
    fn index(self) -> usize {
        self as usize
    }
}

/// `AttributeModifier`.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    /// The modifier's identifier, e.g. `"minecraft:sprinting"`. A bare path (`"sprinting"`) means
    /// the `minecraft` namespace, as for `Identifier.parse`; it is stored in `namespace:path` form.
    pub id: String,
    pub amount: f64,
    pub operation: Operation,
}

/// The identifier of the sprint speed modifier (`LivingEntity.SPRINTING_MODIFIER_ID`).
pub const SPRINTING_MODIFIER_ID: &str = "minecraft:sprinting";

impl Modifier {
    /// `LivingEntity.SPEED_MODIFIER_SPRINTING`: +30% total. The Java source is the `float` literal
    /// `0.3F`, widened to `double` (0.30000001192092896).
    pub fn sprinting() -> Modifier {
        Modifier {
            id: SPRINTING_MODIFIER_ID.to_string(),
            amount: f64::from(0.3_f32),
            operation: Operation::AddMultipliedTotal,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Java hashing
// ---------------------------------------------------------------------------------------------

/// `String.hashCode`: `s[0]*31^(n-1) + ... + s[n-1]` over UTF-16 code units, wrapping in `int`.
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    h
}

/// Split an identifier string like `Identifier.parse`: the first `:` separates namespace and path;
/// no separator, or an empty namespace, means `minecraft`.
fn split_identifier(id: &str) -> (&str, &str) {
    match id.find(':') {
        Some(0) => ("minecraft", &id[1..]),
        Some(i) => (&id[..i], &id[i + 1..]),
        None => ("minecraft", id),
    }
}

/// `Identifier.hashCode`: `31 * namespace.hashCode() + path.hashCode()`.
pub fn identifier_hash(id: &str) -> i32 {
    let (ns, path) = split_identifier(id);
    java_string_hash(ns)
        .wrapping_mul(31)
        .wrapping_add(java_string_hash(path))
}

/// The `namespace:path` form of an identifier string (borrowed when it already is).
pub(crate) fn canonical_id(id: &str) -> std::borrow::Cow<'_, str> {
    match id.find(':') {
        Some(i) if i > 0 => std::borrow::Cow::Borrowed(id),
        _ => {
            let (ns, path) = split_identifier(id);
            std::borrow::Cow::Owned(format!("{ns}:{path}"))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// fastutil Object2ObjectOpenHashMap
// ---------------------------------------------------------------------------------------------

/// `HashCommon.mix(int)`: multiply by the golden-ratio constant, then fold the high half down.
fn mix(x: i32) -> i32 {
    let h = x.wrapping_mul(-1_640_531_527);
    h ^ ((h as u32) >> 16) as i32
}

/// `HashCommon.arraySize(expected, 0.75f)`: the table length for `expected` entries.
fn array_size(expected: usize) -> usize {
    let wanted = (expected as f64 / 0.75).ceil() as u64;
    wanted.max(2).next_power_of_two() as usize
}

/// `HashCommon.maxFill(n, 0.75f)`: how many entries a table of length `n` holds before growing.
fn max_fill(n: usize) -> usize {
    (((n as f64) * 0.75).ceil() as usize).min(n - 1)
}

/// The length a `new Object2ObjectOpenHashMap()` starts with (and never shrinks below):
/// `arraySize(16, 0.75f)`.
const INITIAL_TABLE_LEN: usize = 32;
/// fastutil's `DEFAULT_INITIAL_SIZE`: tables of this length or less never shrink.
const DEFAULT_INITIAL_SIZE: usize = 16;

/// One operation's modifier map: a faithful model of fastutil's open-addressing table (linear
/// probing, backward-shift deletion). Each slot is `0` (empty) or `index + 1` into the owning
/// instance's modifier list; the entries' hashes live in a parallel list passed to each call.
#[derive(Clone, Debug, Default, PartialEq)]
struct Table {
    slots: Vec<u16>,
    size: usize,
}

impl Table {
    /// `computeIfAbsent(operation, ... new Object2ObjectOpenHashMap())`: allocate on first use.
    fn ensure(&mut self) {
        if self.slots.is_empty() {
            self.slots = vec![0; INITIAL_TABLE_LEN];
        }
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// Probe for `id`: `Ok(slot)` if present, `Err(slot)` with the first empty slot otherwise.
    fn find(&self, id: &str, hash: i32, mods: &[Modifier]) -> Result<usize, usize> {
        let mask = self.mask();
        let mut pos = (mix(hash) as u32 as usize) & mask;
        loop {
            let s = self.slots[pos];
            if s == 0 {
                return Err(pos);
            }
            if mods[usize::from(s) - 1].id == id {
                return Ok(pos);
            }
            pos = (pos + 1) & mask;
        }
    }

    /// `put(id, modifier)` for the modifier at `idx`.
    fn put(&mut self, idx: usize, mods: &[Modifier], hashes: &[i32]) {
        self.ensure();
        match self.find(&mods[idx].id, hashes[idx], mods) {
            Ok(pos) => self.slots[pos] = idx as u16 + 1,
            Err(pos) => {
                self.slots[pos] = idx as u16 + 1;
                // fastutil: `if (size++ >= maxFill) rehash(arraySize(size + 1, f))`.
                let before = self.size;
                self.size += 1;
                if before >= max_fill(self.slots.len()) {
                    self.rehash(array_size(self.size + 1), hashes);
                }
            }
        }
    }

    /// `remove(id)`; returns whether the key was present.
    fn remove(&mut self, id: &str, hash: i32, mods: &[Modifier], hashes: &[i32]) -> bool {
        self.ensure();
        match self.find(id, hash, mods) {
            Err(_) => false,
            Ok(pos) => {
                self.size -= 1;
                self.shift_keys(pos, hashes);
                let n = self.slots.len();
                if n > INITIAL_TABLE_LEN && self.size < max_fill(n) / 4 && n > DEFAULT_INITIAL_SIZE
                {
                    self.rehash(n / 2, hashes);
                }
                true
            }
        }
    }

    /// fastutil's `shiftKeys`: close the gap left at `pos` by moving back any later entry of the
    /// same probe run that is allowed to occupy it.
    fn shift_keys(&mut self, mut pos: usize, hashes: &[i32]) {
        let mask = self.mask();
        loop {
            let last = pos;
            pos = (last + 1) & mask;
            loop {
                let s = self.slots[pos];
                if s == 0 {
                    self.slots[last] = 0;
                    return;
                }
                let slot = (mix(hashes[usize::from(s) - 1]) as u32 as usize) & mask;
                let movable = if last <= pos {
                    last >= slot || slot > pos
                } else {
                    last >= slot && slot > pos
                };
                if movable {
                    break;
                }
                pos = (pos + 1) & mask;
            }
            self.slots[last] = self.slots[pos];
        }
    }

    /// fastutil's `rehash`: re-insert every entry, highest old slot first, into a table of
    /// `new_len` slots.
    fn rehash(&mut self, new_len: usize, hashes: &[i32]) {
        let old = std::mem::take(&mut self.slots);
        let mut slots = vec![0_u16; new_len];
        let mask = new_len - 1;
        for &s in old.iter().rev() {
            if s == 0 {
                continue;
            }
            let mut pos = (mix(hashes[usize::from(s) - 1]) as u32 as usize) & mask;
            while slots[pos] != 0 {
                pos = (pos + 1) & mask;
            }
            slots[pos] = s;
        }
        self.slots = slots;
    }

    /// The map's `values()` iteration: the slots from the highest to the lowest.
    fn iter<'a>(&'a self, mods: &'a [Modifier]) -> impl Iterator<Item = &'a Modifier> + 'a {
        self.slots
            .iter()
            .rev()
            .filter(|&&s| s != 0)
            .map(move |&s| &mods[usize::from(s) - 1])
    }

    /// A modifier was removed from the instance's list at `removed`: later indices shift down.
    fn renumber_after_removal(&mut self, removed: usize) {
        for s in &mut self.slots {
            if usize::from(*s) > removed + 1 {
                *s -= 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// AttributeInstance / AttributeMap
// ---------------------------------------------------------------------------------------------

/// `Mth.clamp(double, double, double)`: `d < min ? min : Math.min(d, max)`.
fn mth_clamp(d: f64, min: f64, max: f64) -> f64 {
    if d < min {
        min
    } else {
        java_math_min(d, max)
    }
}

/// `Math.min(double, double)`: NaN-propagating, and `-0.0` is smaller than `0.0`.
fn java_math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 {
        // Both zero: the negative one wins.
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `RangedAttribute.sanitizeValue`: clamp into the attribute's range (NaN becomes the minimum).
fn sanitize(a: Attribute, v: f64) -> f64 {
    let (min, max) = a.range();
    if v.is_nan() {
        min
    } else {
        mth_clamp(v, min, max)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Instance {
    base: f64,
    /// `modifierById`: insertion-ordered (`Object2ObjectArrayMap`); removal shifts the tail down.
    mods: Vec<Modifier>,
    /// `Identifier.hashCode` of each entry of `mods`.
    hashes: Vec<i32>,
    /// `modifiersByOperation`, indexed by [`Operation`].
    tables: [Table; 3],
    /// `getValue()`, recomputed whenever the base or a modifier changes.
    value: f64,
}

impl Instance {
    fn new(a: Attribute) -> Self {
        let mut inst = Instance {
            base: a.player_base(),
            mods: Vec::new(),
            hashes: Vec::new(),
            tables: [Table::default(), Table::default(), Table::default()],
            value: 0.0,
        };
        inst.recompute(a);
        inst
    }

    /// `AttributeInstance.calculateValue`.
    fn recompute(&mut self, a: Attribute) {
        let mods = &self.mods;
        let mut d = self.base;
        for m in self.tables[Operation::AddValue.index()].iter(mods) {
            d += m.amount;
        }
        let mut e = d;
        for m in self.tables[Operation::AddMultipliedBase.index()].iter(mods) {
            e += d * m.amount;
        }
        for m in self.tables[Operation::AddMultipliedTotal.index()].iter(mods) {
            e *= 1.0 + m.amount;
        }
        self.value = sanitize(a, e);
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.mods.iter().position(|m| m.id == id)
    }

    /// `AttributeInstance.removeModifier(Identifier)`.
    fn remove(&mut self, a: Attribute, id: &str) -> bool {
        let Some(i) = self.position(id) else {
            return false;
        };
        let op = self.mods[i].operation.index();
        self.tables[op].remove(id, self.hashes[i], &self.mods, &self.hashes);
        self.mods.remove(i);
        self.hashes.remove(i);
        for t in &mut self.tables {
            t.renumber_after_removal(i);
        }
        self.recompute(a);
        true
    }

    /// `AttributeInstance.addTransientModifier` for an id that is not applied yet.
    fn add(&mut self, a: Attribute, m: Modifier) {
        debug_assert!(self.position(&m.id).is_none());
        let op = m.operation.index();
        self.hashes.push(identifier_hash(&m.id));
        self.mods.push(m);
        let idx = self.mods.len() - 1;
        self.tables[op].put(idx, &self.mods, &self.hashes);
        self.recompute(a);
    }

    /// `AttributeInstance.addOrUpdateTransientModifier`: a modifier with the same id and operation
    /// is replaced where it stands (its place in both orders is kept), otherwise it is added.
    ///
    /// Vanilla leaves the old entry behind in the old operation's map when the operation changes;
    /// that stale-entry quirk is not reproduced: a changed operation removes and re-adds.
    fn put(&mut self, a: Attribute, m: Modifier) {
        match self.position(&m.id) {
            Some(i) if self.mods[i].operation == m.operation => {
                let op = m.operation.index();
                self.mods[i] = m;
                self.tables[op].put(i, &self.mods, &self.hashes);
                self.recompute(a);
            }
            Some(_) => {
                let id = m.id.clone();
                self.remove(a, &id);
                self.add(a, m);
            }
            None => self.add(a, m),
        }
    }
}

/// All of an entity's attribute instances (`AttributeMap`).
#[derive(Clone, Debug, PartialEq)]
pub struct Attributes {
    /// One instance per [`Attribute::ALL`] entry, in that order.
    instances: [Instance; Attribute::ALL.len()],
}

impl Attributes {
    /// The player's attributes at their base values, without modifiers.
    pub fn player() -> Self {
        Self {
            instances: std::array::from_fn(|i| Instance::new(Attribute::ALL[i])),
        }
    }

    fn instance(&self, a: Attribute) -> &Instance {
        &self.instances[a.index()]
    }

    fn instance_mut(&mut self, a: Attribute) -> &mut Instance {
        &mut self.instances[a.index()]
    }

    /// `AttributeInstance.getBaseValue`.
    pub fn base(&self, a: Attribute) -> f64 {
        self.instance(a).base
    }

    /// `AttributeInstance.setBaseValue`.
    pub fn set_base(&mut self, a: Attribute, value: f64) {
        let inst = self.instance_mut(a);
        if value != inst.base {
            inst.base = value;
            inst.recompute(a);
        }
    }

    /// `AttributeInstance.getModifiers`: the applied modifiers in application order (the order the
    /// server lists them in an attribute-update packet).
    pub fn modifiers(&self, a: Attribute) -> &[Modifier] {
        &self.instance(a).mods
    }

    /// `AttributeInstance.getModifier`.
    pub fn modifier(&self, a: Attribute, id: &str) -> Option<&Modifier> {
        let inst = self.instance(a);
        let id = canonical_id(id);
        inst.position(&id).map(|i| &inst.mods[i])
    }

    /// `AttributeInstance.hasModifier`.
    pub fn has_modifier(&self, a: Attribute, id: &str) -> bool {
        self.modifier(a, id).is_some()
    }

    /// Apply a transient modifier, first removing any with the same id (what `setSprinting`,
    /// `AttributeMap.addTransientAttributeModifiers` and `MobEffect.addAttributeModifiers` do).
    pub fn add_modifier(&mut self, a: Attribute, mut modifier: Modifier) {
        modifier.id = canonical_id(&modifier.id).into_owned();
        let inst = self.instance_mut(a);
        inst.remove(a, &modifier.id);
        inst.add(a, modifier);
    }

    /// `AttributeInstance.addOrUpdateTransientModifier`: replace the modifier with this id in
    /// place (keeping its position in the iteration orders), or add it.
    pub fn add_or_update_modifier(&mut self, a: Attribute, mut modifier: Modifier) {
        modifier.id = canonical_id(&modifier.id).into_owned();
        self.instance_mut(a).put(a, modifier);
    }

    /// `AttributeInstance.removeModifier(Identifier)`; returns whether one was applied.
    pub fn remove_modifier(&mut self, a: Attribute, id: &str) -> bool {
        let id = canonical_id(id);
        self.instance_mut(a).remove(a, &id)
    }

    /// `AttributeInstance.removeModifiers`: remove every modifier, one by one in application order.
    pub fn remove_all_modifiers(&mut self, a: Attribute) {
        let ids: Vec<String> = self.instance(a).mods.iter().map(|m| m.id.clone()).collect();
        let inst = self.instance_mut(a);
        for id in ids {
            inst.remove(a, &id);
        }
    }

    /// What `ClientboundUpdateAttributesPacket` carries for one attribute: the base value and the
    /// modifiers in application order.
    pub fn snapshot(&self, a: Attribute) -> (f64, Vec<Modifier>) {
        let inst = self.instance(a);
        (inst.base, inst.mods.clone())
    }

    /// `ClientPacketListener.handleUpdateAttributes` for one attribute: take the server's base
    /// value, drop every modifier the client has (including the ones it applied itself, such as
    /// the sprint modifier) and apply the server's list in order.
    pub fn apply_snapshot(&mut self, a: Attribute, base: f64, modifiers: &[Modifier]) {
        self.set_base(a, base);
        self.remove_all_modifiers(a);
        for m in modifiers {
            self.add_modifier(a, m.clone());
        }
    }

    /// `AttributeInstance.getValue`.
    pub fn value(&self, a: Attribute) -> f64 {
        self.instance(a).value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, amount: f64, operation: Operation) -> Modifier {
        Modifier {
            id: id.to_string(),
            amount,
            operation,
        }
    }

    #[test]
    fn java_string_and_identifier_hashes() {
        // Reference values computed with the JVM.
        assert_eq!(java_string_hash("minecraft"), 695_073_197);
        assert_eq!(java_string_hash("sprinting"), 1_429_809_032);
        assert_eq!(java_string_hash(""), 0);
        // Non-ASCII characters hash as UTF-16 code units (here a surrogate pair).
        assert_eq!(java_string_hash("\u{1F600}"), 55_357 * 31 + 56_832);
        assert_eq!(identifier_hash("minecraft:sprinting"), 1_502_241_659);
        assert_eq!(identifier_hash("minecraft:effect.speed"), -1_372_016_227);
        assert_eq!(identifier_hash("minecraft:effect.slowness"), 822_227_848);
        assert_eq!(identifier_hash("minecraft:powder_snow"), 986_390_332);
        // A bare path is in the minecraft namespace.
        assert_eq!(
            identifier_hash("sprinting"),
            identifier_hash("minecraft:sprinting")
        );
        assert_eq!(
            identifier_hash(":sprinting"),
            identifier_hash("minecraft:sprinting")
        );
    }

    #[test]
    fn player_base_values_widen_floats() {
        let a = Attributes::player();
        assert_eq!(
            a.value(Attribute::MovementSpeed).to_bits(),
            0.100_000_001_490_116_12_f64.to_bits()
        );
        assert_eq!(
            a.value(Attribute::JumpStrength).to_bits(),
            0.419_999_986_886_978_15_f64.to_bits()
        );
        assert_eq!(a.value(Attribute::Gravity), 0.08);
        assert_eq!(a.value(Attribute::StepHeight), 0.6);
        assert_eq!(a.value(Attribute::MaxHealth), 20.0);
    }

    #[test]
    fn operations_fold_in_game_order() {
        let mut a = Attributes::player();
        let at = Attribute::Armor;
        a.set_base(at, 4.0);
        a.add_modifier(at, m("t:add", 2.0, Operation::AddValue));
        a.add_modifier(at, m("t:base", 0.5, Operation::AddMultipliedBase));
        a.add_modifier(at, m("t:total", 0.25, Operation::AddMultipliedTotal));
        // d = 4 + 2 = 6; e = 6 + 6 * 0.5 = 9; e *= 1.25 -> 11.25
        assert_eq!(a.value(at), 11.25);
        assert!(a.remove_modifier(at, "t:base"));
        assert!(!a.remove_modifier(at, "t:base"));
        assert_eq!(a.value(at), 7.5);
    }

    #[test]
    fn values_are_clamped_to_the_attribute_range() {
        let mut a = Attributes::player();
        a.add_modifier(
            Attribute::MovementSpeed,
            m("t:slow", -5.0, Operation::AddMultipliedTotal),
        );
        assert_eq!(a.value(Attribute::MovementSpeed), 0.0);
        a.set_base(Attribute::Armor, f64::NAN);
        assert_eq!(a.value(Attribute::Armor), 0.0);
        a.set_base(Attribute::MaxHealth, 5000.0);
        assert_eq!(a.value(Attribute::MaxHealth), 1024.0);
    }

    #[test]
    fn sprint_speed_stack_order_matches_the_recorded_game() {
        // effect_speed and legacy_capture: speed II (+0.2F * 2) with sprinting (+0.3F), then
        // slowness I (-0.15F) as well. The expected bit patterns are the oracle's `attrs` values.
        let mut a = Attributes::player();
        let ms = Attribute::MovementSpeed;
        a.add_modifier(
            ms,
            m(
                "minecraft:effect.speed",
                f64::from(0.2_f32) * 2.0,
                Operation::AddMultipliedTotal,
            ),
        );
        assert_eq!(
            a.value(ms).to_bits(),
            0.140_000_002_682_209_02_f64.to_bits()
        );
        a.add_modifier(ms, Modifier::sprinting());
        assert_eq!(a.value(ms).to_bits(), 0.182_000_005_155_801_8_f64.to_bits());
        a.add_modifier(
            ms,
            m(
                "minecraft:effect.slowness",
                f64::from(-0.15_f32),
                Operation::AddMultipliedTotal,
            ),
        );
        assert_eq!(
            a.value(ms).to_bits(),
            0.154_700_003_297_626_98_f64.to_bits()
        );
        // The fold runs slot-descending: sprinting (slot 23), speed (19), slowness (2).
        let order: Vec<&str> = a.instance(ms).tables[2]
            .iter(&a.instance(ms).mods)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(
            order,
            [
                "minecraft:sprinting",
                "minecraft:effect.speed",
                "minecraft:effect.slowness"
            ]
        );
    }

    #[test]
    fn fold_order_is_observable_in_the_last_bit() {
        // effect_speed rows 165..: speed III (+0.2F * 3), slowness I (-0.15F) and sprinting
        // (+0.3F). The oracle's value is the product in the game's table order (sprinting, speed,
        // slowness); the same factors in another order land one ulp away.
        let ms = Attribute::MovementSpeed;
        let mut a = Attributes::player();
        a.add_modifier(
            ms,
            m(
                "minecraft:effect.speed",
                f64::from(0.2_f32) * 3.0,
                Operation::AddMultipliedTotal,
            ),
        );
        a.add_modifier(
            ms,
            m(
                "minecraft:effect.slowness",
                f64::from(-0.15_f32),
                Operation::AddMultipliedTotal,
            ),
        );
        a.add_modifier(ms, Modifier::sprinting());
        assert_eq!(
            a.value(ms).to_bits(),
            0.176_800_004_003_942_02_f64.to_bits()
        );
        let base = f64::from(0.1_f32);
        let speed = 1.0 + f64::from(0.2_f32) * 3.0;
        let slow = 1.0 + f64::from(-0.15_f32);
        let sprint = 1.0 + f64::from(0.3_f32);
        assert_eq!(
            ((base * sprint) * speed * slow).to_bits(),
            a.value(ms).to_bits()
        );
        assert_ne!(
            ((base * speed) * slow * sprint).to_bits(),
            a.value(ms).to_bits()
        );
    }

    #[test]
    fn re_adding_a_modifier_replaces_it() {
        let mut a = Attributes::player();
        a.add_modifier(Attribute::Armor, m("t:x", 1.0, Operation::AddValue));
        a.add_modifier(Attribute::Armor, m("t:y", 2.0, Operation::AddValue));
        a.add_modifier(Attribute::Armor, m("t:x", 4.0, Operation::AddValue));
        assert_eq!(a.value(Attribute::Armor), 6.0);
        // Re-adding moves the modifier to the end of the application order.
        let ids: Vec<&str> = a
            .modifiers(Attribute::Armor)
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["t:y", "t:x"]);
    }

    #[test]
    fn snapshot_replaces_client_modifiers() {
        let mut a = Attributes::player();
        let ms = Attribute::MovementSpeed;
        a.add_modifier(ms, Modifier::sprinting());
        let snapshot = vec![m(
            "minecraft:effect.speed",
            0.4,
            Operation::AddMultipliedTotal,
        )];
        a.apply_snapshot(ms, a.base(ms), &snapshot);
        assert!(!a.has_modifier(ms, SPRINTING_MODIFIER_ID));
        assert!(a.has_modifier(ms, "minecraft:effect.speed"));
        assert_eq!(a.snapshot(ms).1, snapshot);
    }

    /// Cases generated with the real fastutil 8.5.18 `Object2ObjectOpenHashMap`, keyed by
    /// objects with `Identifier`'s hash: a random put/remove sequence over a pool of keys (see
    /// [`pool_id`]) and the `values()` iteration order afterwards, as pool indices. Some cases
    /// grow the table past its first resize threshold (25 entries) and shrink it back.
    const FASTUTIL_CASES: &str = "\
+20 -4 +22 +17 -6 | 22 20 17
-12 |
-22 +18 +15 +20 +19 -20 -16 -11 | 18 15 19
+7 +15 +10 +6 +5 -9 +13 +16 -4 -12 | 6 15 5 10 16 13 7
+2 -4 -21 +21 -21 +13 -9 | 13 2
+17 -22 | 17
-16 +12 +13 -14 +6 -10 +8 -9 -14 -10 | 12 6 13 8
-22 +12 -1 +12 +3 -19 +23 -22 +16 +1 | 12 23 1 3 16
+17 -10 -21 | 17
+9 +17 +0 -7 | 9 0 17
+9 +8 +20 -4 -6 +16 +15 -7 +0 | 9 0 15 20 16 8
+1 +10 +13 -15 -11 -12 +14 -12 +5 | 1 5 10 13 14
+0 +13 +7 +19 -23 +17 +18 -10 -12 | 18 0 19 17 13 7
+18 +2 -8 -15 +7 -11 -13 +22 -6 | 18 22 2 7
-18 -15 -1 -0 +19 -15 -13 -17 -7 -9 -13 +16 | 19 16
-11 -3 -0 -7 |
+10 -3 +18 +12 | 12 18 10
-6 +2 -5 | 2
-20 +3 +18 -13 | 18 3
+8 +17 -8 -23 -23 +0 -6 | 0 17
-7 +21 +4 | 21 4
-19 +19 +0 -3 -4 +0 +3 +20 +7 -8 +17 | 0 3 20 19 17 7
+20 +11 +8 | 20 11 8
+18 +10 -16 +16 +14 -1 +9 | 9 18 10 16 14
-7 +10 -16 -4 +1 +2 -8 -3 | 1 10 2
-11 +3 -11 | 3
+11 -22 | 11
+13 | 13
-21 +8 -1 -2 +19 +16 -2 | 19 16 8
-22 -1 -11 -18 +12 +16 +20 +9 -15 -14 +21 -23 | 12 9 21 20 16
+5 +18 -20 +18 +5 -7 -23 +3 +16 +2 +9 -21 | 9 18 3 5 16 2
+12 -9 -20 +12 +10 +10 -7 -17 +17 +6 | 12 6 10 17
-1 -10 -3 -8 |
-18 -8 -5 -21 +5 +23 +14 -3 -21 -3 -2 -7 | 23 5 14
+14 -10 +17 +19 +10 -6 -2 | 10 19 17 14
-21 -11 +20 -20 -12 +18 +13 -9 +9 -20 | 9 18 13
-16 -5 -19 -7 |
-6 +0 +11 -0 -16 +20 +2 -2 -17 | 20 11
+12 -0 +10 -8 +23 +14 +3 -7 +13 -13 | 12 23 3 10 14
+1 | 1
+10 -20 -12 +2 -14 +23 +19 -18 -2 | 23 10 19
+19 -3 -23 +12 -8 | 12 19
+7 -14 -1 -19 -12 -1 +11 -14 +5 | 5 11 7
-16 +14 -0 -4 +22 -13 +13 -21 +3 | 3 22 13 14
+0 -11 +11 +8 -8 +5 +5 +2 -23 +8 | 0 5 11 2 8
+11 | 11
-18 +17 +17 -14 | 17
-2 +21 -10 -17 +7 +22 +7 +19 +6 -10 -7 | 6 22 21 19
+21 +10 +0 -0 | 21 10
-10 +18 +6 -1 -23 +16 +6 +4 +7 +12 -7 | 12 18 6 4 16
+8 -19 -0 -13 +15 | 15 8
+13 +7 +9 +18 +21 +0 -2 +0 +21 +7 | 9 18 0 21 13 7
-0 -7 -13 -3 -10 -15 -0 +2 -0 +10 +0 +2 | 0 10 2
-6 +1 -17 -8 | 1
+4 +20 -13 -22 -10 +4 -18 +23 -4 -12 +12 +13 | 12 23 20 13
+5 -20 +11 +15 -15 -20 -3 +5 -6 | 5 11
-3 +20 -19 +0 -0 +19 +15 +15 | 15 20 19
+19 +2 +20 +19 -15 -15 +5 +17 +13 -14 -4 +14 | 5 20 19 13 17 2 14
+13 +1 +18 +14 -22 +23 +4 -13 -5 | 18 23 1 4 14
+22 -2 -23 +9 -0 +16 +17 -16 +10 +15 -20 +12 | 12 9 15 22 10 17
+15 -16 +15 -5 -0 +6 -18 -13 +12 | 12 6 15
-7 -11 |
+12 -9 -16 -2 -6 +3 +23 +16 -19 +12 -9 | 12 23 3 16
-8 -23 +14 -3 -21 -2 -12 +6 +12 -5 -0 +20 | 12 6 20 14
+15 -16 +18 +19 +13 +2 +7 +23 | 18 23 15 19 2 13 7
-20 +12 -22 +22 -19 +13 | 12 22 13
+12 +1 -4 +10 +18 +15 -8 -23 +19 | 12 18 1 15 10 19
+18 -18 -0 +8 +9 | 9 8
+0 -21 +14 -14 +5 +2 | 0 5 2
-20 +5 -7 +18 | 18 5
-21 +3 -20 -14 -12 | 3
+14 +11 +8 +17 | 11 17 14 8
+23 -15 +18 -12 +8 -14 +11 +21 +19 -11 +21 -22 | 18 23 21 19 8
+12 +2 +5 -1 -11 +7 | 12 5 2 7
+18 +8 +9 -0 -19 +20 -12 -14 -20 | 9 18 8
-22 +14 | 14
+12 -12 -11 +21 -16 | 21
+19 -4 -19 +3 -20 | 3
+20 -18 +1 -20 +11 +9 | 9 1 11
+22 +11 +13 -2 | 22 11 13
+11 | 11
+2 +2 -3 -1 | 2
+15 -18 -14 -19 +4 -8 +19 +9 +1 -11 +18 +9 | 9 18 1 15 4 19
+3 +18 -13 | 18 3
-5 +4 -11 -1 +10 -7 +6 | 6 10 4
-1 +17 +4 -4 -19 -10 +14 -14 | 17
-0 -18 -21 +8 -23 +3 | 3 8
+8 -18 +22 -3 -21 -7 -12 -18 +0 | 0 22 8
-5 +18 +2 -4 -21 +17 | 18 17 2
-9 +0 | 0
+39 +33 +28 +33 -14 +3 +10 +2 -8 +5 +29 +5 +1 +6 +40 -20 -32 +20 -39 | 28 1 40 6 33 5 3 29 10 20 2
+8 -29 +21 +14 +3 -45 +13 +45 -13 -21 +22 +31 -27 -47 +27 +14 +10 -34 +29 +27 +33 -45 +8 +5 +33 +42 +35 +18 +38 -32 -3 -13 -35 -12 -24 -17 -22 | 31 42 18 33 27 5 29 10 38 14 8
+15 +30 +13 +30 +34 +30 +19 +41 +8 +37 -39 +41 -14 -25 -25 +44 +4 +31 +43 +2 +29 +0 +14 +12 +43 +6 +9 +13 +23 +31 +18 +28 +22 +9 -33 +0 +20 -36 -36 -3 -35 -35 -14 +11 -33 +33 +37 -22 -46 -31 | 12 9 41 6 33 15 29 4 19 11 13 2 43 18 30 0 44 28 23 20 37 34 8
+47 +33 +14 +25 +45 +9 +15 +47 -26 +25 +42 +11 +4 +44 +7 +27 +14 +15 +3 +43 +29 +16 +10 +45 -8 +2 -30 +18 +7 -7 +14 +31 -5 +36 +31 -25 +14 +0 -6 +24 +1 +36 +44 +23 +46 +43 +24 -17 -40 +19 +40 +22 +35 -11 +40 +35 +6 +34 +8 -29 +42 +30 +9 -41 -34 -39 -1 -27 -19 -8 +22 -15 -18 -21 -47 -34 -45 -34 -9 -40 +38 -12 | 24 46 6 33 3 10 4 16 2 14 31 42 43 30 0 44 23 22 35 38 36
+28 +17 +23 -28 +43 -33 +0 -37 -26 -33 -31 +17 +19 +8 -14 +32 -47 -47 +27 -37 +30 +18 +17 +5 -12 -14 -3 +5 +28 +4 +33 -31 +33 +36 -1 -45 -47 +19 +23 -28 -14 -21 -44 +42 -7 +34 +10 +24 -4 +36 +13 +25 +19 +23 -3 +16 +20 +40 -13 +37 -22 +22 -27 -0 -45 +20 -3 +12 -15 -4 -11 +5 -1 +44 +26 +26 -46 +37 -26 | 36 12 42 43 18 30 44 23 25 40 33 22 5 10 20 19 32 16 37 17 34 24 8
+16 +6 +24 -11 +4 +45 +2 -14 +10 -6 -39 +44 +19 +17 +10 +28 +33 +0 +31 +41 +41 +5 +28 +16 +3 +24 +25 +32 +40 +47 +32 +40 +19 +20 +13 +27 +17 -2 +14 +31 +16 +24 +19 +9 +35 +3 +46 +2 +43 +23 -36 -20 -7 -28 -37 -9 +27 +39 +42 -10 -46 -24 +24 -29 -1 -32 | 24 41 25 40 33 5 3 4 19 2 16 45 17 13 14 31 42 43 0 47 44 23 27 39 35
+43 +38 +46 +8 -2 +13 +44 +34 +46 +0 +9 +22 +29 +37 -28 +11 +45 +6 +13 +26 +15 +14 +12 -6 +6 -29 -31 | 12 9 43 0 44 26 46 6 15 22 14 11 38 37 34 13 45 8
+46 +20 +44 +33 +38 -28 +22 +28 +42 +5 +28 +43 +31 +30 +37 +36 +45 +37 +26 +0 +47 +42 +23 -6 +17 +32 +40 +4 +16 +44 -44 +22 +29 +19 +21 +28 -42 +16 +5 +8 +42 -27 -19 +29 +5 +18 -35 +1 +1 +35 +33 +11 +41 +47 -38 +21 +42 +9 -33 -16 -0 -40 -28 -33 -4 -14 -13 -20 +33 -15 -24 -1 -40 -24 -38 | 36 9 41 26 46 33 5 29 11 17 45 31 42 43 18 30 47 23 21 22 35 32 37 8
+15 +13 -15 +12 -16 +4 +0 +14 +15 -0 | 12 15 4 13 14
+31 -36 +17 -11 +16 -6 +27 +37 +6 +17 +18 +1 +40 +0 +14 +6 +44 +2 +24 +41 +45 -8 +10 +22 -23 -5 +3 +39 +9 +44 +16 +25 +17 +21 +20 -14 +19 +9 +16 +6 +2 +1 +10 +36 -5 +10 +42 -14 -41 -24 -20 -28 -1 +28 -24 -13 -3 -32 -2 -29 -4 -11 | 36 9 6 25 40 10 19 16 17 45 31 42 18 0 28 44 22 27 21 39 37
+41 -40 +43 -16 +1 -3 -6 +6 -46 +19 +12 +25 +13 +1 -4 +40 -3 -46 +19 -41 -32 -7 -3 +30 -45 +2 -45 -2 +31 -15 -34 -11 -3 +41 +43 -7 +18 -21 +14 +17 -28 +32 +42 -26 -30 +19 -23 -34 +30 -32 -40 -46 -27 +35 -42 +17 -47 -19 +4 +5 +46 -16 -23 +1 +39 -20 +45 +18 -15 +4 -12 +19 -26 -30 -18 -23 -4 -26 -23 +11 -33 -11 +0 +23 | 31 43 41 0 23 1 46 25 6 5 39 19 35 45 17 13 14
-29 +24 +43 +34 +22 +6 +4 +34 +22 -27 +6 +34 +9 +19 +16 +25 +17 +17 +13 -25 -30 +21 +47 -44 -39 +10 -16 -1 | 24 9 43 47 6 21 22 10 4 19 13 17 34
+21 +28 +13 +5 +43 +2 +30 +27 -34 +30 +42 +7 -23 -26 +34 +0 -12 -28 -9 -47 +1 -29 | 42 43 30 0 1 27 5 21 34 2 13 7
+5 -18 +12 +42 +27 +38 -31 +28 +23 +17 +32 +24 +15 +5 -29 +0 +28 +37 +8 +25 +25 +7 +37 +24 +34 +16 +41 +5 +6 +11 +28 +11 +37 +2 -7 +21 -19 +34 +23 -34 +34 +11 +23 +2 +27 +30 +41 +7 +5 +44 +14 -20 +44 -29 -9 +27 -42 -20 -46 +24 -45 -9 -10 +9 -16 -38 -32 -19 -6 -41 | 24 12 9 25 15 5 11 17 2 14 30 0 28 44 23 27 21 37 34 8 7
+14 +22 +8 +11 +34 -14 +16 +13 +22 +33 +28 +14 +10 +20 +6 +37 +7 +20 +20 +22 +46 +40 +23 +44 -46 -17 -19 +13 -40 -11 +22 -47 -39 | 44 28 23 6 33 22 10 20 7 16 37 13 34 14 8
+10 +46 -28 +2 -30 -5 +2 -7 +23 +46 -31 -27 -12 -27 +22 +6 +27 +23 +35 | 23 46 6 27 22 10 35 2
-31 +26 +14 +46 -11 -35 -7 +28 +46 +13 | 28 26 46 13 14
+34 +15 +12 -8 +25 +32 +15 +5 +5 -29 +13 +38 +2 +27 +10 +32 +24 +11 +33 +15 +40 +22 +24 +42 +30 +0 +1 +17 -31 +23 +37 -0 +26 +2 -25 -43 +41 +4 +26 +34 +30 +24 +34 +12 +8 +20 +3 +13 +45 +24 -24 +18 +15 +30 +40 +32 +26 +25 +21 +10 +1 +39 -22 -42 +5 -17 -3 -16 -24 +26 +46 -28 -12 -5 -8 -6 -21 -43 -39 -10 -22 -38 -11 -35 +45 -7 | 41 46 26 25 40 33 15 4 13 2 45 18 30 23 1 27 20 32 37 34
+47 +27 +29 +23 +7 +37 +6 +39 -45 +4 -35 +23 +11 +19 +37 +35 +29 +31 +16 +27 +47 +26 +13 +17 +2 -22 -18 +38 +24 +5 +27 +17 +35 +0 +37 +39 +9 +12 +45 +3 +34 +5 +20 +47 +8 +23 -29 -9 -37 -1 +31 -18 +42 -0 -22 +5 -46 -9 -32 -2 -14 -7 -20 | 24 12 26 6 3 5 4 19 11 16 17 13 45 31 42 47 23 27 39 35 38 34 8
+17 +45 +37 -46 -44 -37 -27 +40 -41 +12 +14 +8 -34 -41 +37 -27 -27 +11 +33 -6 +33 -44 +45 +39 -12 -2 -46 -40 +30 +27 +19 +29 +2 -14 -4 -34 +32 +46 +5 +47 +5 -24 +17 +22 -45 +33 -32 -33 -11 -26 -19 +38 -13 -14 -5 -36 +26 +16 -9 -5 -5 +32 +32 -28 +9 -4 -18 +22 -44 +0 -17 +27 | 9 30 0 47 26 46 27 22 29 39 32 16 38 37 2 8
+32 +11 +27 -41 -27 +7 -2 +32 +15 +20 -38 -1 +9 +47 -5 +37 -44 -43 +10 +13 -6 -40 +39 +20 +32 -14 +42 +17 -44 +16 -25 -19 -39 -44 +7 -37 +38 -39 -47 +25 +26 +20 -21 +31 +46 +11 -42 -13 -37 +19 -6 -37 -30 -26 +14 +4 +34 -41 -45 +46 +2 -2 -12 +37 +27 -35 +44 -17 +3 -3 -23 +23 -43 -2 -5 -36 +12 -26 -35 -17 -44 +18 +27 | 12 31 9 18 23 46 25 27 15 4 19 10 38 20 11 32 16 37 34 14 7
+24 +39 +32 +24 +25 -18 -16 -36 +1 +7 +18 -35 +28 -18 -39 +47 +2 +9 +11 -26 +1 +46 +3 -3 -4 +12 -46 -17 +32 +39 +6 -8 -20 -46 -46 +17 -22 -26 +3 -19 -10 +14 -44 -15 -12 +38 +43 -45 -34 -42 +11 +17 +12 -42 -18 +10 -38 -0 +46 -47 -19 -19 +25 | 24 12 43 9 28 1 46 25 6 3 10 39 11 32 17 2 14 7
-1 |
-7 -30 -8 -17 -5 +11 +44 -40 -25 -6 +41 -33 -22 +43 -36 -37 +22 -29 -46 +20 -32 +42 +17 -39 +33 +25 +34 -6 +36 -45 +31 +45 -12 -33 -33 +38 -23 -11 +8 +20 +10 -43 -35 -19 -9 +10 -23 -27 -13 -13 -5 -12 +45 +18 -38 -33 +40 -15 -16 -47 -7 +40 -32 +4 -13 +23 +14 +10 +8 +19 +46 -3 +15 +12 +11 +44 +3 -42 -12 | 36 31 18 41 44 23 46 25 40 3 15 22 10 4 20 19 11 14 17 34 45 8
+0 | 0
+17 +3 +43 +19 +37 +13 +3 +3 +27 -15 +17 +20 +2 +38 +41 +19 +18 -28 -9 +36 +30 +5 +23 -41 +1 -43 +5 +26 +35 +39 -46 -26 -25 -30 +13 -46 -35 -27 -35 | 36 18 23 1 5 3 39 20 19 38 2 37 17 13
+6 +37 +38 +10 +35 +25 +2 +31 +16 +5 +34 +17 -25 -47 -15 | 31 6 5 10 17 35 16 38 34 37 2
-40 +27 +27 +46 +20 +31 -47 +14 +18 -47 +41 +34 +12 +46 +10 +26 +5 +7 +41 +13 +26 +33 +25 +32 +34 +26 +38 +29 +16 +45 +37 +46 -29 -46 +9 +33 +32 -9 -37 -8 -10 -38 -9 +17 -23 +33 -0 -26 | 12 31 18 41 25 33 27 5 20 17 32 16 45 13 34 14 7
+14 -20 -25 +21 +10 +16 +2 +0 +35 +24 +32 -15 +29 -4 +21 +35 -26 +1 -32 -32 | 24 0 1 21 29 10 35 16 2 14
-13 +41 +9 +46 +31 +46 +22 +23 +18 +43 -16 +22 +47 -30 +12 -9 -38 +40 +1 +32 +38 +25 +7 +0 -47 +7 +24 +45 +38 +40 +17 -45 +33 +18 +34 +12 +38 +2 +41 -18 +0 -1 +27 +14 +42 +16 +27 +14 +33 +13 +32 -36 +5 +11 -19 -34 +8 +46 +14 +30 +13 +25 +46 -15 -32 -40 -42 -7 -11 -37 -1 -18 -28 -10 +4 +7 +44 -31 -27 | 24 12 41 46 25 33 5 4 16 17 2 13 14 43 30 0 44 23 22 38 7 8
+35 +41 +15 +34 +14 +23 +0 +47 +24 +5 +28 +43 +2 +38 +30 +40 +20 +27 +31 +25 +6 +19 +7 +29 +26 +10 +18 +1 +45 +17 +42 +22 -34 -30 -41 -17 -31 -14 -47 -40 -26 -42 -22 -0 -20 -25 -7 -2 -35 -27 -18 -29 -43 -15 | 24 28 23 1 6 5 10 19 38 45
+23 +24 +34 +22 +16 +30 +26 +25 +10 +8 +32 +5 +41 +0 +47 +43 +18 +31 +28 +27 +6 +21 +38 +12 +20 +14 +15 +42 +39 -41 -27 -42 -18 -34 -0 -12 -10 -26 -21 -23 -15 -28 -24 -39 -6 -32 -5 -25 -31 -38 +44 -25 | 43 30 47 44 22 20 16 14 8
+13 +4 +24 +47 +17 +19 +26 +45 +5 +43 +42 +11 +16 +22 +31 +21 +18 +9 +37 +32 +29 +0 +2 +8 +27 +14 -2 -11 -29 -5 -9 -0 -31 -45 -32 -17 -13 -24 -37 -16 -8 -43 -19 -27 -21 +4 +30 -2 +18 +21 +22 -30 | 42 18 47 26 21 22 4 14
+29 +34 +22 +18 +10 +16 +6 +19 +35 +45 +13 +38 +15 +14 +2 +11 +33 +44 +42 +9 +7 +31 +8 +30 +41 +1 +26 +37 +40 +17 +0 +20 +36 +12 +46 +43 +25 +28 +4 +24 -34 -18 -4 -37 -2 -22 -7 -35 -45 -13 -26 -9 -43 -19 -20 -36 -8 -0 -29 -46 -14 -15 -25 -10 -17 -12 -38 -42 -24 -41 -11 -40 -44 -28 | 31 30 1 6 33 16
+47 +33 +0 +38 +18 +36 +16 +12 +11 +39 +40 +30 +23 +27 +7 +46 +22 +42 +45 +5 +17 +1 +31 +10 +43 +29 +6 +9 +32 +25 +34 +24 +28 +37 +35 +15 +13 +8 +21 +26 +20 +14 +19 -34 -19 -15 -27 -12 -47 -28 -40 -35 -33 -37 -26 -46 -22 -38 -9 -31 -45 -13 -0 -43 -8 -29 -24 -16 -25 -21 -5 -30 -7 -10 -17 -1 -6 -42 -20 -36 -14 | 18 23 39 32 11
+7 +27 +11 +44 +4 +19 +31 +5 +35 +6 +30 +47 +23 +38 +12 +21 +13 +37 +29 +39 +10 +45 +28 +43 +40 +46 +24 +0 +2 +32 +15 +34 +3 +22 -45 -35 -27 -15 -21 -24 -47 -22 -13 -38 -6 -44 -11 -10 -3 -46 -4 -12 -43 -39 -31 -7 -37 -29 -34 -5 -19 -2 -40 -23 -22 -40 -15 +18 +47 +0 -18 -9 +40 | 30 47 0 28 40 32
+40 +43 +6 +38 +26 +34 +25 +29 +33 +20 +42 +14 +9 +32 +18 +30 +12 +0 +21 +2 +5 +24 +17 +23 +35 +13 +3 +47 +11 +27 +22 +4 +37 +10 +39 +45 +19 +1 +16 +8 +46 +44 -19 -20 -43 -26 -13 -34 -1 -23 -16 -0 -46 -17 -38 -11 -14 -9 -33 -4 -24 -21 -5 -37 -6 -44 -22 -12 -25 -29 -42 -2 -18 -40 +38 | 30 47 27 3 10 39 35 32 38 45 8
+2 +24 +5 +41 +8 +40 +47 +29 +18 +19 +33 +39 +9 +0 +36 +1 +6 +22 +13 +10 +25 +16 +34 +23 +7 +42 +11 +27 +3 +46 +20 +31 +32 +21 +12 +30 +44 +26 +4 +15 +43 +37 -33 -18 -7 -44 -20 -11 -30 -41 -29 -13 -39 -32 -22 -42 -46 -23 -2 -26 -5 -21 -9 -6 -19 -3 -36 -15 -40 -43 -37 -8 -25 -47 -34 -12 +41 -29 -5 +8 +3 +2 -5 -8 +17 | 24 31 41 0 1 27 3 10 4 16 17 2
+45 +9 +15 +29 +21 +31 +4 +13 +0 +3 +42 +2 +33 +24 +6 +28 +20 +30 +5 +27 +40 +34 +22 +10 +46 +12 +25 +37 +41 +14 +19 +7 +43 +36 +39 +18 +44 +32 +16 -4 -5 -21 -6 -30 -28 -13 -7 -24 -46 -37 -9 -16 -32 -45 -36 -29 -10 -19 -25 -44 -20 -3 -31 -41 -27 -43 -22 -2 -40 -33 -42 +46 +46 +13 +17 -0 | 12 18 46 15 39 17 13 34 14
+11 +45 +8 +33 +31 +42 +34 +21 +41 +5 +27 +16 +3 +7 +18 +26 +20 +29 +4 +10 +24 +23 +14 +38 +32 +30 +39 +12 -41 -23 -5 -21 -33 -30 -45 -12 -16 -29 -24 -20 -31 -14 -11 -39 -34 -4 -10 -26 +21 | 42 18 21 27 3 32 38 7 8
+33 +28 +24 +36 +18 +2 +13 +23 +35 +46 +3 +7 +27 +30 +11 +17 +12 +43 +34 +29 +25 +47 +9 +10 +21 +44 -36 -46 -25 -47 -33 -43 -44 -12 -29 -28 -34 -23 -17 -10 -35 -30 -24 -35 +26 | 9 18 26 21 27 3 11 13 2 7
+7 +0 +4 +1 +25 +40 +32 +41 +34 +45 +24 +11 +16 +33 +36 +27 +17 +35 +30 +12 +44 +2 +28 +31 +42 +43 +15 +26 +22 +23 +39 +10 +29 +38 +21 +6 +19 +37 +46 -19 -2 -1 -25 -27 -38 -44 -6 -12 -26 -42 -36 -7 -15 -33 -32 -35 -30 -28 -10 -23 -24 -29 -45 -4 -46 -34 -41 -11 -16 -37 -39 -40 -15 -34 +10 -2 +3 | 31 43 0 3 22 21 10 17
";

    /// The pool the cases were generated over: the real modifier ids, then synthetic keys in three
    /// namespaces.
    fn pool_id(i: usize) -> String {
        const REAL: [&str; 8] = [
            "minecraft:sprinting",
            "minecraft:effect.speed",
            "minecraft:effect.slowness",
            "minecraft:powder_snow",
            "minecraft:effect.jump_boost",
            "minecraft:effect.health_boost",
            "minecraft:base_speed",
            "minecraft:armor.chest",
        ];
        const NS: [&str; 3] = ["minecraft", "t", "mod"];
        if i < REAL.len() {
            REAL[i].to_string()
        } else {
            format!("{}:k{i}", NS[i % 3])
        }
    }

    #[test]
    fn open_hash_table_matches_fastutil() {
        let mut cases = 0;
        let mut grew = 0;
        for line in FASTUTIL_CASES.lines() {
            let (ops, expected) = line.split_once('|').expect("case format");
            let mut inst = Instance::new(Attribute::Armor);
            let mut peak = 0;
            for op in ops.split_whitespace() {
                let i: usize = op[1..].parse().unwrap();
                if op.starts_with('+') {
                    inst.put(Attribute::Armor, m(&pool_id(i), 1.0, Operation::AddValue));
                } else {
                    inst.remove(Attribute::Armor, &pool_id(i));
                }
                peak = peak.max(inst.mods.len());
            }
            let table = &inst.tables[Operation::AddValue.index()];
            let got: Vec<String> = table.iter(&inst.mods).map(|m| m.id.clone()).collect();
            let want: Vec<String> = expected
                .split_whitespace()
                .map(|i| pool_id(i.parse().unwrap()))
                .collect();
            assert_eq!(got, want, "case {cases}: {line}");
            assert_eq!(table.size, inst.mods.len(), "case {cases}");
            cases += 1;
            if peak > 24 {
                grew += 1;
            }
        }
        assert!(cases >= 100, "{cases} cases");
        assert!(grew >= 10, "only {grew} cases grew the table");
    }

    /// Cases generated with the real 1.21.11 `AttributeInstance` (movement speed, base `0.1F`):
    /// random add (`+id,operation,amount`, replacing an existing modifier of the same id) and
    /// remove (`-id`) sequences, and the bits of `getValue()` after each step. The ids come from a
    /// small pool so that several share a slot of the 32-entry hash tables; see [`value_pool_id`]
    /// and [`VALUE_AMOUNTS`].
    const VALUE_CASES: &str = "\
+10,1,16 +1,0,2 -2 -0 -2 -1 -0 +8,1,11 | 3fb9893752158106 3fdfeb85269ae148 3fdfeb85269ae148 3fdfeb85269ae148 3fdfeb85269ae148 3fb9893752158106 3fb9893752158106 3fb322d0ea158106
+0,2,17 -2 -5 -7 +6,1,9 -3 +10,2,8 +3,2,13 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb70a3d76666666 3fb70a3d76666666 3fb9581068a3d70a 3fa9581068a3d70a
+3,0,12 +1,0,8 +11,0,4 | 3fe3333334000000 3fe6666667333333 3fe1999997333333
-11 -9 +1,0,12 +3,2,4 +8,0,9 -10 +6,1,16 | 3fb99999a0000000 3fb99999a0000000 3fe3333334000000 3fe051eb83e147ae 3fdb3333315c28f6 3fdb3333315c28f6 3fdb21cabe9889a0
+8,2,16 -7 +6,0,1 -1 | 3fb9893752158106 3fb9893752158106 3fd326e97d9020c5 3fd326e97d9020c5
+2,1,8 +4,2,17 +9,0,0 -1 +7,0,15 | 3fbc28f5c999999a 3fbc28f5c999999a 3fdc28f5d2666666 3fdc28f5d2666666 3e4fae147a000000
+5,0,12 +6,2,5 | 3fe3333334000000 3fdae147a7851eb8
+3,2,12 -8 +11,0,10 -0 +9,0,0 | 3fc3333338000000 3fc3333338000000 3fe0ccccce000000 3fe0ccccce000000 3fef33333e000000
+5,2,8 +6,0,1 +10,1,3 +4,2,10 -2 +3,0,10 +8,1,0 | 3fbc28f5c999999a 3fd51eb857333334 3fe0e56047581063 3fe51eb8592e147c 3fe51eb8592e147c 3ff35c28fa370a3e 3ff6fd70ab35c290
-3 +8,2,15 +5,1,16 +4,1,9 +1,0,0 | 3fb99999a0000000 3faeb851f3333333 3faea4a8c8e69ad4 3fab923a3094af4e 3fcb923a3932617d
+1,1,17 +1,2,4 -5 +7,2,6 -1 +10,0,0 +0,0,15 -10 -0 | 3fb99999a0000000 3fb5c28f5f0a3d70 3fb5c28f5f0a3d70 3fb6d9168a3126e9 3fbae147b4cccccd 3fdae147bd333334 3e4e3d70a3000000 0 3fbae147b4cccccd
+4,1,6 -1 -8 | 3fbae147b4cccccd 3fbae147b4cccccd 3fbae147b4cccccd
-4 +5,0,2 +4,0,17 -4 | 3fb99999a0000000 3fe0000004000000 3fe0000004000000 3fe0000004000000
+1,1,4 | 3fb5c28f5f0a3d70
-2 -5 -6 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000
+2,1,0 -3 +1,1,10 +0,0,14 -6 | 3fc0a3d710f5c290 3fc0a3d710f5c290 3fc3d70a44f5c290 3ffb47ae189eb852 3ffb47ae189eb852
-11 | 3fb99999a0000000
+5,2,12 +1,2,1 +1,2,7 +1,2,1 +1,0,1 +6,2,12 +0,0,14 +11,0,6 +2,2,11 | 3fc3333338000000 3fc70a3d775c28f6 3fc23d70a8666666 3fc70a3d775c28f6 3fdcccccd4000000 3fe599999f000000 4007666667c00000 40084cccce266667 400239999a9ccccd
-11 +4,0,16 | 3fb99999a0000000 3fb8f5c295000000
-1 -3 -10 +8,2,10 +7,0,17 -3 +1,2,2 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fc0000004000000 3fc0000004000000 3fc0000004000000 3fc666666d99999a
-6 +0,0,7 +7,2,4 -9 +0,1,13 +4,0,15 | 3fb99999a0000000 3fa99999a6666666 3fa5c28f647ae146 3fa5c28f647ae146 3fa5c28f5f0a3d70 0
+8,1,6 +5,2,16 -4 +5,0,14 +2,2,10 -2 +3,0,8 +2,1,10 -9 | 3fbae147b4cccccd 3fbad013afc9c77a 3fbad013afc9c77a 3ff27ae14819999a 3ff719999a200000 3ff27ae14819999a 3ff428f5c2fae148 3ff8f5c28fe147ae 3ff8f5c28fe147ae
-5 -9 +1,2,13 +2,0,17 +6,2,2 +4,1,2 -5 +3,1,8 | 3fb99999a0000000 3fb99999a0000000 3fa99999a0000000 3fa99999a0000000 3fb1eb85247ae148 3fb9168734dd2f1c 3fb9168734dd2f1c 3fbae147b8831270
-8 +10,2,8 +4,0,5 +10,1,5 | 3fb99999a0000000 3fbc28f5c999999a 0 0
+4,0,12 | 3fe3333334000000
+3,1,0 +9,2,14 +11,1,2 +3,2,1 +5,2,1 +10,2,9 +6,2,13 -6 -7 | 3fc0a3d710f5c290 3fd0a3d710f5c290 3fd5c28f6570a3d8 3fd581062cac0832 3fd9ce0769e1b08b 3fd7396d127e5217 3fc7396d127e5217 3fd7396d127e5217 3fd7396d127e5217
+3,2,12 | 3fc3333338000000
+3,0,8 -3 +1,2,8 +5,2,13 | 3fc999999ccccccd 3fb99999a0000000 3fbc28f5c999999a 3fac28f5c999999a
+1,2,8 +1,1,2 +1,1,12 -6 | 3fbc28f5c999999a 3fc1eb85247ae148 3fc3333338000000 3fc3333338000000
-1 +7,2,12 +9,2,2 +11,2,4 +7,2,15 -5 -6 +5,2,3 | 3fb99999a0000000 3fc3333338000000 3fcae147b6b851ec 3fc6d9168bd2f1aa 3fb247453ca8c154 3fb247453ca8c154 3fb247453ca8c154 3fbd3ed53098b2ea
-10 +2,1,1 +7,0,15 +5,0,5 -8 +2,0,10 +5,2,17 +5,0,7 +10,0,12 | 3fb99999a0000000 3fbeb851f47ae148 0 0 0 0 0 0 3fd999999b333333
+4,0,16 -7 -4 | 3fb8f5c295000000 3fb8f5c295000000 3fb99999a0000000
-11 -6 -9 -8 +5,0,5 +6,2,5 -2 -6 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 0 0 0 0
+8,2,3 +7,1,10 +2,0,9 | 3fc47ae14eb851ec 3fc99999a2666667 3e2999999a666666
+1,2,8 -2 +10,0,0 -0 -6 +6,0,11 | 3fbc28f5c999999a 3fbc28f5c999999a 3fdc28f5d2666667 3fdc28f5d2666667 3fdc28f5d2666667 3fc51eb87199999a
-11 -0 -0 -10 +8,0,3 +7,2,6 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fe666666c000000 3fe7851ebe333333
-1 -8 -5 -9 +9,2,5 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb1eb851e147ae0
-2 +10,1,5 | 3fb99999a0000000 3fb1eb851e147ae0
-0 +6,0,2 +9,0,11 -5 +1,2,6 +5,1,9 | 3fb99999a0000000 3fe0000004000000 3fd0000008000000 3fd0000008000000 3fd0ccccd5333333 3fce3d70b2f5c290
+6,2,10 | 3fc0000004000000
+0,1,10 +8,2,15 +5,0,3 +6,0,3 -0 | 3fc0000004000000 3fb3333338000000 3fe0ccccd1000000 3fef33333b000000 3fe8f5c295999999
+10,0,4 | 0
-5 +11,0,3 -4 -6 +7,2,14 +7,1,7 +9,2,14 +9,0,6 | 3fb99999a0000000 3fe666666c000000 3fe666666c000000 3fe666666c000000 3ff666666c000000 3fe547ae19cccccd 3ff547ae19cccccd 3fe6ccccd21eb852
+9,0,7 +7,0,14 +11,2,15 | 3fa99999a6666666 3ff0cccccd333333 3fe428f5c30a3d70
+9,0,15 +11,0,0 +6,2,17 +4,1,8 +3,1,2 -1 -2 -6 | 0 3e4ccccccc000000 3e4ccccccc000000 3e4fae147a000000 3e5599999a70a3d7 3e5599999a70a3d7 3e5599999a70a3d7 3e5599999a70a3d7
+10,1,16 +3,1,3 +1,1,1 +7,0,6 +11,0,5 | 3fb9893752158106 3fc472b027c3126f 3fc7020c52008313 3fd141893c104189 0
-7 +8,0,4 +10,2,5 | 3fb99999a0000000 0 0
-3 +5,1,10 -0 -10 +8,1,15 -5 +2,2,14 +7,0,15 | 3fb99999a0000000 3fc0000004000000 3fc0000004000000 3fc0000004000000 3fb5c28f6199999a 3faeb851f3333333 3fbeb851f3333333 0
-10 | 3fb99999a0000000
+8,0,1 +8,1,1 | 3fd3333338000000 3fbeb851f47ae148
+1,0,2 +3,2,0 +8,2,17 +4,0,8 | 3fe0000004000000 3fe4ccccd5333334 3fe4ccccd5333334 3fe8f5c298666667
+4,0,16 +8,2,15 +8,0,10 -10 +7,1,13 +9,2,10 -8 +7,1,9 -1 | 3fb8f5c295000000 3fadf3b64c666666 3fd63d70a5400000 3fd63d70a5400000 3fc63d70a5400000 3fcbccccce900000 3faf33333a400000 3fbc147ae7a00000 3fbc147ae7a00000
+4,2,2 -6 | 3fc1eb85247ae148 3fc1eb85247ae148
+1,2,2 -2 -6 +3,2,6 -6 -1 +11,0,10 -7 +8,1,4 | 3fc1eb85247ae148 3fc1eb85247ae148 3fc1eb85247ae148 3fc2d0e5664dd2f2 3fc2d0e5664dd2f2 3fbae147b4cccccd 3fd7851eba000000 3fd7851eba000000 3fd3fdf3b5591687
+8,2,7 +3,1,10 +2,0,0 +9,1,8 +11,1,9 +10,0,14 | 3fb851eb8b333333 3fbe66666e000000 3fde666677800000 3fe06a7f03170a3d 3fde666677800000 3ffa99999de00000
+5,1,13 +9,0,1 -5 +10,2,5 -6 | 3fa99999a0000000 3fc3333338000000 3fd3333338000000 3fcae147ad1eb850 3fcae147ad1eb850
+7,2,10 +1,1,12 +4,0,17 +6,2,14 +7,2,0 +10,1,5 -4 | 3fc0000004000000 3fc8000006000000 3fc8000006000000 3fd8000006000000 3fd8f5c29970a3d8 3fd3f7ceddd2f1a9 3fd3f7ceddd2f1a9
-8 +10,2,0 -11 +4,1,4 +5,0,15 +5,1,13 | 3fb99999a0000000 3fc0a3d710f5c290 3fc0a3d710f5c290 3fbc49ba664dd2f1 0 3fa74bc6aab020c3
+4,0,11 +2,1,2 +11,2,4 | 0 0 0
+8,1,4 -4 +10,0,7 -2 +6,1,0 +7,1,3 +6,0,14 -8 +6,2,9 | 3fb5c28f5f0a3d70 3fb5c28f5f0a3d70 3fa5c28f647ae146 3fa5c28f647ae146 3fad70a3e851eb86 3fb6666674ccccce 3ff85c28f72e147a 3ffae147b13d70a4 3fb26e9798418938
+5,2,7 | 3fb851eb8b333333
-6 +5,1,11 -7 -1 -8 | 3fb99999a0000000 3fb3333338000000 3fb3333338000000 3fb3333338000000 3fb3333338000000
+4,2,4 +8,1,2 +3,0,8 | 3fb5c28f5f0a3d70 3fbe76c8ba6e978d 3fce76c8b69fbe76
+3,2,12 +1,1,9 +9,2,15 +11,1,9 | 3fc3333338000000 3fc147ae18cccccc 3fb4bc6a8428f5c1 3fb26e9791eb851e
+8,0,11 +3,0,9 +4,1,0 -7 | 0 0 0 0
+4,1,16 +11,0,8 +7,2,16 +0,2,14 +8,0,14 -9 | 3fb9893752158106 3fc989374ee45a1d 3fc978df7d56c615 3fd978df7d56c615 40031aa79c03a31b 40031aa79c03a31b
+3,2,11 +6,1,15 -3 +10,0,7 | 3fb3333338000000 3fa70a3d76666666 3faeb851f3333333 3f9eb851fae147ad
+4,0,2 -8 -8 +10,2,11 -2 +1,0,15 -0 -2 +2,1,16 | 3fe0000004000000 3fe0000004000000 3fe0000004000000 3fd8000006000000 3fd8000006000000 3fb333334b333332 3fb333334b333332 3fb333334b333332 3fb326e990b70a3c
+8,0,7 +6,1,17 | 3fa99999a6666666 3fa99999a6666666
-9 +4,0,1 +2,0,15 | 3fb99999a0000000 3fd3333338000000 0
+7,2,9 +7,2,14 +1,1,8 +11,2,15 -4 | 3fb70a3d76666667 3fc99999a0000000 3fcc28f5c999999a 3fc0e56045c28f5c 3fc0e56045c28f5c
+5,0,3 +1,2,16 | 3fe666666c000000 3fe6581067d2d0e5
+3,0,13 +7,1,12 | 0 0
+0,1,0 | 3fc0a3d710f5c290
+3,2,3 | 3fc47ae14eb851ec
-3 +4,2,10 +6,1,12 +2,0,7 +2,0,4 | 3fb99999a0000000 3fc0000004000000 3fc8000006000000 3fb800000bffffff 0
+2,2,14 -1 +8,0,5 +5,2,0 | 3fc99999a0000000 3fc99999a0000000 0 0
+6,2,1 -10 +9,1,3 +1,0,7 +5,2,11 +9,1,5 | 3fbeb851f47ae148 3fbeb851f47ae148 3fc89374c5e353f9 3fb89374cc083128 3fb26e97990624de 3fa020c49fc6a7ed
-1 -6 -0 +8,0,2 +9,2,2 +11,2,6 +6,0,1 +6,1,14 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fe0000004000000 3fe666666d99999a 3fe7851ebfe147ae 3ff076c8b9841894 3ff7851ebfe147ae
+11,0,0 +8,0,16 +8,1,13 | 3fd99999a8000000 3fd970a3e5400000 3fc99999a8000000
+5,0,4 +0,1,7 +1,0,1 | 0 0 3fc23d70a0cccccd
+9,1,17 | 3fb99999a0000000
-9 -7 -5 -2 +6,1,9 +11,2,10 -9 +9,0,8 +9,2,10 | 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fb70a3d76666666 3fbcccccd4000000 3fbcccccd4000000 3fccccccd0666666 3fc2000004800000
+3,1,7 +9,2,8 -10 -9 +3,1,8 +1,2,0 | 3fb851eb8b333333 3fbac083191eb852 3fbac083191eb852 3fb851eb8b333333 3fbc28f5c999999a 3fc24dd2f90e5605
+2,2,8 -4 +7,1,9 | 3fbc28f5c999999a 3fbc28f5c999999a 3fb9581068a3d70a
+6,0,8 +10,1,0 +8,1,17 +2,1,6 -5 -6 | 3fc999999ccccccd 3fd0a3d70ee147ae 3fd0a3d70ee147ae 3fd147ae19333333 3fd147ae19333333 3fc147ae1b5c28f6
+10,2,2 +0,0,5 -1 +2,0,13 | 3fc1eb85247ae148 0 0 0
+2,2,11 -4 +9,2,5 +10,0,2 +0,0,17 +9,0,13 +7,0,11 | 3fb3333338000000 3fb3333338000000 3faae147ad1eb850 3fd0cccccc333332 3fd0cccccc333332 3e38000000000000 0
+10,0,14 -4 -0 | 3ff199999a000000 3ff199999a000000 3ff199999a000000
+1,0,9 -4 | 3e19999998000000 3e19999998000000
-10 +6,0,9 +6,1,2 -8 | 3fb99999a0000000 3e19999998000000 3fc1eb85247ae148 3fc1eb85247ae148
+5,2,9 +10,0,2 +8,0,14 +4,2,12 +4,0,15 +3,0,16 | 3fb70a3d76666667 3fdcccccd4000000 3ff599999b666667 40003333348ccccd 3fefae147e7ae149 3fef9ba5e6d7ae15
+4,0,7 | 3fa99999a6666666
+10,2,5 | 3fb1eb851e147ae0
+2,1,16 | 3fb9893752158106
-10 +1,2,16 +5,1,10 -11 +6,2,11 -8 +10,0,0 +5,0,15 -0 | 3fb99999a0000000 3fb9893752158106 3fbfeb85269ae148 3fbfeb85269ae148 3fb7f0a3dcf428f6 3fb7f0a3dcf428f6 3fd7f0a3e46f5c29 3e458bc6a745fbe7 3e458bc6a745fbe7
+3,1,3 +11,1,15 +11,2,7 +7,0,10 | 3fc47ae14eb851ec 3fbeb851f70a3d72 3fc374bc712f1aa0 3fe10624dffef9db
-4 +9,1,15 -5 +0,1,3 +8,2,16 +0,2,3 +4,0,4 -4 | 3fb99999a0000000 3faeb851f3333333 3faeb851f3333333 3fbeb851f70a3d72 3fbea4a8ccbb2fee 3fb883ba3c9e6eec 0 3fb883ba3c9e6eec
+10,0,9 +10,0,17 +0,0,15 +2,0,4 -11 +6,1,6 +10,0,6 -6 | 3e19999998000000 3fb99999a0000000 0 0 0 0 0 0
+11,0,11 +3,1,12 -1 +5,1,12 +1,2,10 +5,1,14 +8,0,10 | 0 0 0 0 0 0 3fd4000005000000
+11,1,13 -3 +5,0,4 +6,1,10 +4,2,0 +1,1,9 +0,0,7 | 3fa99999a0000000 3fa99999a0000000 0 0 0 0 0
-5 -9 +10,1,12 -10 -2 -11 +10,1,0 -10 | 3fb99999a0000000 3fb99999a0000000 3fc3333338000000 3fb99999a0000000 3fb99999a0000000 3fb99999a0000000 3fc0a3d710f5c290 3fb99999a0000000
+7,0,10 -7 | 3fd6666668000000 3fb99999a0000000
-3 +10,0,1 -6 -2 -3 -4 -10 +4,1,6 | 3fb99999a0000000 3fd3333338000000 3fd3333338000000 3fd3333338000000 3fd3333338000000 3fd3333338000000 3fb99999a0000000 3fbae147b4cccccd
-10 +11,2,7 +8,1,0 | 3fb99999a0000000 3fb851eb8b333333 3fbf9db239d2f1ab
-9 | 3fb99999a0000000
+0,2,10 | 3fc0000004000000
+10,0,13 +1,1,15 | 0 0
+6,2,2 +8,0,15 +0,0,8 +8,2,9 +6,1,1 | 3fc1eb85247ae148 0 0 3fd020c49ed0e561 3fcba5e3589374bd
+6,1,16 +11,1,8 +8,1,17 -1 +8,0,6 +7,2,6 -4 | 3fb9893752158106 3fbc18937baf1aa0 3fbc18937baf1aa0 3fbc18937baf1aa0 3fc5126e9b01cac0 3fc6202755f514e3 3fc6202755f514e3
-5 +14,0,10 -36 -16 -5 +34,1,1 +8,1,8 +4,1,10 -5 +30,1,16 +24,1,17 +18,0,11 +9,1,8 -36 -29 +1,2,4 +13,2,15 +7,2,16 +9,1,2 -28 +15,2,10 +25,2,6 +33,2,9 +39,2,2 +31,0,10 -16 +5,2,2 +12,2,7 +6,0,0 +9,2,7 +35,1,4 +14,1,5 +34,0,3 +21,2,15 +25,1,9 +34,2,7 +34,0,1 +20,2,8 +33,2,1 | 3fb99999a0000000 3fd6666668000000 3fd6666668000000 3fd6666668000000 3fd6666668000000 3fdae147b11eb852 3fdd1eb8551eb852 3fe15c28f78f5c29 3fe15c28f78f5c29 3fe154fdf57a0c4a 3fe154fdf57a0c4a 3fc3ced91c14fdf4 3fc5168730e1cac1 3fc5168730e1cac1 3fc5168730e1cac1 3fc1ecbfb43dab9f 3fb5827fa516cdf2 3fb574bb7c4ae14a 3fb95ced2b3ae3eb 3fb95ced2b3ae3eb 3fbfb42876099ce4 3fc0a4fba45ea591 3fbdf5c4f4aa5d3a 3fc4f8d6acc38b1b 3fe259bbd3e4382e 3fe259bbd3e4382e 3fe9b0d3c41547d4 3fe867fc60add109 3ff6a9b3849221d5 3ff11b93afca7c3a 3feee61fd3962730 3fdddd9e7731d099 3fee876cbcf2f307 3fe251413e2b5e9e 3fdf00b6f3becfe2 3fc78fe7388b5be1 3fd29a076098ec47 3fd4763b50a8371b 3fdb484f1758a21b
+27,0,8 +5,2,3 -22 -21 +18,0,8 +35,0,6 -38 +24,2,11 +33,0,12 +10,1,4 -28 -18 +23,0,17 +34,2,5 -24 +14,0,12 +35,2,15 +36,2,3 +12,0,15 | 3fc999999ccccccd 3fd47ae14c28f5c3 3fd47ae14c28f5c3 3fd47ae14c28f5c3 3fdeb851f0f5c290 3fe1eb8521ae147b 3fe1eb8521ae147b 3fdae147b2851eb8 3ff051eb872147ae 3febbe76c8db645a 3febbe76c8db645a 3fe87ae147e9374c 3fe87ae147e9374c 3fe122d0e0a44674 3fe6d91680db089a 3ff30a3d6b20346d 3fe5ef1fd79652be 3ff18c197aea3161 3fe765774ecbfb14
-12 -7 +16,2,2 +15,1,7 +15,2,7 -39 +26,2,7 +36,1,5 -27 -32 +4,2,9 -11 +20,1,1 +31,0,12 -4 +13,1,8 -29 -34 +16,1,4 -4 +18,2,15 +21,1,7 -34 -30 +35,0,0 -24 +8,2,16 | 3fb99999a0000000 3fb99999a0000000 3fc1eb85247ae148 3fc10624e2a7ef9e 3fc10624e2a7ef9e 3fc10624e2a7ef9e 3fc02c3ca41f8a09 3fb6a454df4da8fe 3fb6a454df4da8fe 3fb6a454df4da8fe 3fb460b2c8f91817 3fb460b2c8f91817 3fba332f05834f3d 3fe3a663400a7c16 3fe5d551d560fba7 3fe8425aed7dee76 3fe8425aed7dee76 3fe8425aed7dee76 3fdd75253e9ae147 3fdd75253e9ae147 3fd1acaff25ced91 3fd0a2877a638865 3fd0a2877a638865 3fd0a2877a638865 3fd8f3cb3cc816ee 3fd8f3cb3cc816ee 3fd8e3d30c8e39bf
+19,1,8 +21,2,8 +8,2,6 +13,1,5 +9,1,6 -36 +1,1,8 +15,1,13 +39,2,15 -36 +17,0,11 -6 +12,1,10 -25 -0 -33 -12 +16,0,7 -38 -27 -38 -9 +10,0,13 +34,0,4 -19 +36,2,17 +20,2,7 -29 -6 +34,1,14 +13,0,3 -1 | 3fbc28f5c999999a 3fbef9db2a8f5c2a 3fc0432ca98b4396 3fb7a786c226809d 3fb921ff2ea786c2 3fb921ff2ea786c2 3fbc16f007a9930c 3faa9c77953eab37 3f9feef5e64b33db 3f9feef5e64b33db 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
+27,1,7 -14 +39,2,12 -34 -26 +6,1,1 -26 +2,2,12 +15,2,2 +0,1,4 +37,1,10 +38,2,10 +15,1,15 +34,1,11 -8 +34,1,4 +30,0,3 -3 | 3fb851eb8b333333 3fb851eb8b333333 3fc23d70a8666666 3fc23d70a8666666 3fc23d70a8666666 3fc6147ae7c28f5c 3fc6147ae7c28f5c 3fd08f5c2dd1eb85 3fd72f1aa8343958 3fd428f5c8083127 3fd933333a4ac084 3fdf800008dd70a6 3fce99999f733334 3fc599999d333334 3fc599999d333334 3fc933333419999a 3ff60ccccd966665 3ff60ccccd966665
+1,1,11 -26 -29 +27,1,4 -26 +28,1,1 +37,1,4 +38,2,5 +30,0,8 -16 +9,0,10 -39 -25 -16 +12,2,13 +33,1,8 +35,1,17 -26 +6,1,11 +1,2,17 +9,2,17 +17,1,11 +7,2,1 -19 +1,0,15 +20,0,12 +2,1,11 -25 +16,2,7 -36 +18,0,3 +10,1,15 -21 -19 +7,1,12 +13,2,2 | 3fb3333338000000 3fb3333338000000 3fb3333338000000 3faeb851ee147ae0 3faeb851ee147ae0 3fb47ae14b851eb8 3fb0a3d70a8f5c28 3fa74bc6a1ba5e34 3fb74bc69ed0e561 3fb74bc69ed0e561 3fca353f70d91687 3fca353f70d91687 3fca353f70d91687 3fca353f70d91687 3fba353f70d91687 3fbe3d7096d4fdf4 3fbe3d7096d4fdf4 3fbe3d7096d4fdf4 3fb428f5b7df3b64 3fbe3d7096d4fdf4 3faae147a4624dd3 3fa1eb851676c8b5 3fa581061bda511a 3fa581061bda511a 0 3fb020c49437b4a1 3fa020c48f611340 3fa020c48f611340 3f9ea4a8aa053e2d 3f9ea4a8aa053e2d 3fb6fb7e8211a75c 0 0 0 3fbad0139b39f558 3fc2c4da87995fec
+7,2,7 -14 -18 +10,1,9 +20,0,13 +4,0,0 -0 +12,2,0 +10,1,2 -21 +28,2,5 -23 -26 +3,1,8 -12 +34,2,16 -32 +21,0,1 +37,2,9 -19 +29,0,1 -9 +12,2,3 -37 -9 +13,1,7 -37 +8,2,15 +36,1,8 +7,0,8 +17,2,6 -28 +16,0,16 +5,0,0 +34,0,10 | 3fb851eb8b333333 3fb851eb8b333333 3fb851eb8b333333 3fb5e353fd47ae14 0 0 0 0 0 0 0 0 0 0 0 0 0 3fb978dfbab31738 3fb6ecc95b3ac819 3fb6ecc95b3ac819 3fd13196e7c41aaf 3fd13196e7c41aaf 3fdb828b0f013488 3fde910c498f8fb4 3fde910c498f8fb4 3fdd8c368b717f90 3fdd8c368b717f90 3fd1ba8720774c8a 3fd2f3879e34f981 3fda992a1a35a3ed 3fdbed9f6851ec1f 3fe3f2df995b5642 3fe3d2f499f398cc 3ff1648e2564c487 3ff7af9cb68a07a6
+21,2,2 +1,1,9 +18,0,16 +34,1,1 +20,1,3 | 3fc1eb85247ae148 3fc020c4a0d4fdf4 3fbf731905aa9932 3fc3381d84561e50 3fcdb3d088d89376
+12,2,7 +33,0,8 -19 +26,0,10 +34,2,6 +22,1,5 -11 -31 -2 +22,0,0 +3,2,15 +38,0,14 -22 -35 -24 +4,2,13 +24,1,6 +35,2,5 +32,1,11 +16,1,14 -10 +18,1,13 | 3fb851eb8b333333 3fc851eb8828f5c2 3fc851eb8828f5c2 3fdb5c28f747ae14 3fdcba5e36d81062 3fd41c0eba3ec56d 3fd41c0eba3ec56d 3fd41c0eba3ec56d 3fd41c0eba3ec56d 3fe7f0a3de38d4fe 3fdcba5e3dddcc64 3ff0c20c4be1f213 3febc538efaf6944 3febc538efaf6944 3febc538efaf6944 3fdbc538efaf6944 3fdd28aefbab61bb 3fd4694743efd49d 3fcf1a5436c2ca14 3fe17ecf5ecd91ac 3fe17ecf5ecd91ac 3fd945646c7e4431
+11,0,11 +10,2,15 +1,2,11 -38 +29,1,4 | 0 0 0 0 0
+5,0,4 +37,1,1 +27,0,7 -1 +13,1,7 +21,1,15 +13,2,2 +5,2,16 +34,1,4 +39,2,10 +20,0,9 +1,0,17 +39,2,1 -3 +37,1,8 | 0 0 0 0 0 0 0 3fac99aea45ac474 3fa73cdde24c56d6 3fad0c155adf6c8b 0 0 0 0 0
+34,2,7 +12,0,12 +38,1,10 +34,0,17 +11,1,10 -34 -13 +11,2,7 -36 +31,2,17 +4,2,2 -14 +22,2,11 -26 +18,2,16 -38 +26,2,4 +4,0,16 +18,0,16 +27,2,17 +29,0,8 -26 +30,0,1 +3,2,0 +11,0,13 +33,0,12 +18,0,14 -25 +0,0,11 +31,0,1 +15,2,6 +28,2,12 -6 +36,0,4 +6,1,16 | 3fb851eb8b333333 3fe23d70a4999999 3fe6cccccdc00000 3fe8000001000000 3fecccccce000000 3fecccccce000000 3fecccccce000000 3fe6cccccdc00000 3fe6cccccdc00000 3fe6cccccdc00000 3fefeb8522547ae0 3fefeb8522547ae0 3fe7f0a3d9bf5c29 3fe7f0a3d9bf5c29 3fe7e151854ac468 3fe31aa79dd569ec 3fe03d0e7772bc79 3fd719e86212bc99 3fd70fec54dc617c 3fd70fec54dc617c 3fdaf02ddd9ef0d7 3fdfb126ea575c28 3fe467ef9f4547ad 3fea871decbba6b4 3fd8a5e35c11cac0 3febec8b4a08e561 3ffd99db287e624f 3ffd99db287e624f 3ff9b374c17e624f 3ffcd22d14ac76ca 3ffe4315bc1b7cbb 4006b2504d149d8c 4006b2504d149d8c 4004da91d4c8d6e4 4004cd3925787d0a
-25 -24 +18,2,0 +17,1,16 +28,0,9 +4,0,16 -24 -38 -8 -18 +15,0,7 +6,2,5 +17,1,11 -9 +30,0,7 +29,2,15 +15,1,7 +20,1,1 +17,2,8 -10 +26,0,14 +27,1,7 +13,2,17 +30,0,11 +1,2,14 -24 +29,1,3 +3,0,10 +14,2,14 -30 -2 +22,1,1 +23,1,4 +26,1,15 +0,2,2 +32,0,16 +27,2,12 -8 +0,2,2 -28 | 3fb99999a0000000 3fb99999a0000000 3fc0a3d710f5c290 3fc09930c4b54c99 3e209930bf856d5d 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 3fe01be87573abaf 3fded1372388b34f 3fded1372388b34f 3fd84ff2161b0271 3fe84ff2161b0271 3fe84ff2161b0271 3fff4fb00774f909 4004e44281d30ffa 4014e44281d30ffa 401a20acffeba36e 401a20acffeba36e 401d33942d7e917c 401ae566ca5a76b4 3ff07736cc89a122 3ff70d4cb9996711 3ff6d1b0cc27d3f4 4001bf898245d617 4001bf898245d617 4001bf898245d617 4008fe0ac68e2172
+14,2,1 -22 +9,1,6 +11,2,0 +39,1,8 +5,1,13 -25 +35,2,11 +19,1,13 +39,2,3 +31,2,9 -7 -23 -18 +15,0,10 +9,0,11 +35,1,0 +37,1,0 +23,0,5 +30,1,2 +21,2,10 | 3fbeb851f47ae148 3fbeb851f47ae148 3fc020c4a05a1cac 3fc4f76606e21966 3fc6f69450ae7d58 3fb9f559bf611343 3fb9f559bf611343 3fb378034f88ce71 3f91f8a0982f837d 3f832b55f97635ea 3f81409a2d50ca1f 3f81409a2d50ca1f 3f81409a2d50ca1f 3f81409a2d50ca1f 3f9e310dc9e9318a 0 3fb1409a38d130e8 3fc1409a38d130e8 0 0 0
-39 +10,1,13 +38,1,4 -13 -5 +20,2,10 +23,1,12 +26,1,5 +22,2,14 +22,2,5 -20 +5,2,8 -36 +22,2,7 +2,2,10 +9,2,11 +30,0,4 | 3fb99999a0000000 3fa99999a0000000 3fa1eb851e147ae0 3fa1eb851e147ae0 3fa1eb851e147ae0 3fa6666665999998 3fbb333336cccccc 3fb1999994666664 3fc1999994666664 3fa8a3d6fbeb851e 3fa3b64596560418 3fa5aee6255e9e1a 3fa5aee6255e9e1a 3fad6d5cf1fb15b2 3fb2645a173ced8f 3fab968722db6456 0
+19,1,15 -24 +34,1,5 +4,1,5 -27 +14,1,11 +3,1,4 -4 +7,0,7 +22,0,10 +14,0,13 +20,1,4 -12 +34,2,0 | 3faeb851f3333333 3faeb851f3333333 3f9eb851deb851e6 0 0 0 0 0 0 0 0 3e347ae146000000 3e347ae146000000 0
-2 +12,2,2 -37 -3 +20,0,1 -25 +12,1,16 +29,2,11 -3 | 3fb99999a0000000 3fc1eb85247ae148 3fc1eb85247ae148 3fc1eb85247ae148 3fdae147b6b851ec 3fdae147b6b851ec 3fd326e97d9020c5 3fccba5e3c583128 3fccba5e3c583128
+17,0,0 +16,1,7 +27,0,3 +31,1,16 +16,0,11 -36 +37,2,4 +16,1,1 +25,1,14 -24 +10,2,14 +16,0,9 +11,1,0 -7 -3 +21,0,10 -22 +10,1,7 +14,1,3 -11 +26,2,14 -36 -17 +39,2,7 +15,2,4 +6,1,10 -31 +35,1,7 -7 +2,0,7 +37,2,2 +34,2,17 -13 -28 -9 | 3fd99999a8000000 3fd851eb92cccccd 3fee666671cccccd 3fee51eb90651eb8 3fe7f0a3e2f051eb 3fe7f0a3e2f051eb 3fe45958181a9ba4 3ff04937509b5c28 3ffde2d0edb4f5c2 3ffde2d0edb4f5c2 400de2d0edb4f5c2 4008730be82518fb 400c1f141cd7c91d 400c1f141cd7c91d 400c1f141cd7c91d 4011f76949b05c5d 4011f76949b05c5d 40019350b626c2c3 40064477a1c63fb0 4003ebe42afc43c9 4013ebe42afc43c9 4013ebe42afc43c9 400d72f83c00f489 400bfa056c341b81 4007c7b7cc6016d6 400a1d2385db02c3 400a231ceb93f972 4009aba0f9ae970e 4009aba0f9ae970e 4008290ffa5d51ac 4013e594b3d3b00b 4013e594b3d3b00b 4013e594b3d3b00b 4013e594b3d3b00b 4013e594b3d3b00b
+22,2,15 +33,2,11 -29 -36 +33,1,4 +38,2,17 -1 -3 +15,2,13 +28,1,0 +7,2,15 -5 +34,1,3 +35,2,10 -15 +24,1,12 -28 -19 +18,0,0 +29,2,6 +15,2,12 -28 -0 +33,2,1 +16,0,16 +20,0,7 +35,0,17 +20,1,4 -38 | 3faeb851f3333333 3fa70a3d76666666 3fa70a3d76666666 3fa70a3d76666666 3faa1cac0ba5e353 3faa1cac0ba5e353 3faa1cac0ba5e353 3faa1cac0ba5e353 3f9a1cac0ba5e353 3fa1a9fbed604189 3f95326183404ea4 3f95326183404ea4 3fa020c4a1fbe76d 3fa428f5ca7ae148 3fb428f5ca7ae148 3fb9eb8528147ae1 3fb676c8ba6a7efa 3fb676c8ba6a7efa 3fd676c8c16f9db1 3fd79652cb1b98c8 3fe1b0be1854b296 3fe1b0be1854b296 3fe1b0be1854b296 3fe6dc8764656820 3fe6b7f3587ab626 3fe3dc626d898aa6 3fdfc703e275aaa2 3fe0e06ba00a5d40 3fe0e06ba00a5d40
-10 +17,2,14 | 3fb99999a0000000 3fc99999a0000000
+16,2,13 +35,1,6 +27,1,8 +29,0,12 +8,1,3 -13 -5 -30 +30,2,7 -6 +38,0,3 -3 +26,2,9 -6 -15 +35,0,9 +10,1,8 -12 -31 +28,0,11 -33 -29 +11,2,12 +17,2,13 -37 -39 +13,2,12 +39,1,7 -8 +17,2,12 -5 +6,2,3 | 3fa99999a0000000 3faae147b4cccccd 3fad70a3de666667 3fd6147ae2333333 3fe0cccccef0a3d7 3fe0cccccef0a3d7 3fe0cccccef0a3d7 3fe0cccccef0a3d7 3fdfeb8522c9374b 3fdfeb8522c9374b 3fefeb85261c6a7e 3fefeb85261c6a7e 3fecba5e3be65fd8 3fecba5e3be65fd8 3fecba5e3be65fd8 3fe994e3c32710cb 3feb161e56075f70 3feb161e56075f70 3feb161e56075f70 3fe4ee2eb7d1de6a 3fe4ee2eb7d1de6a 3fd13c9ef6cdb8bb 3fd9daee72349518 3fc9daee72349518 3fc9daee72349518 3fc9daee72349518 3fd36432d5a76fd2 3fd2da4dddfc7fcc 3fc8c72480b7212d 3fe2955b608958e2 3fe2955b608958e2 3fedbbc56a0bc250
+13,2,3 +17,0,15 +5,1,8 -23 +10,1,15 +1,2,0 -23 +21,1,13 +8,2,9 +37,1,1 +38,0,12 +8,1,17 -2 +15,0,4 -29 +23,2,12 -2 -19 -25 +26,0,9 -23 -24 +34,0,12 +2,2,4 +10,1,12 +2,2,3 +38,2,14 +19,2,5 -6 | 3fc47ae14eb851ec 0 0 0 0 0 0 0 0 0 3fc32b55f8a9bcfe 3fc54c9869a02753 3fc54c9869a02753 3fa54c984703afad 3fa54c984703afad 3faff2e46a858783 3faff2e46a858783 3faff2e46a858783 3faff2e46a858783 0 0 0 3fd7f62b6fd70a3f 3fd45e0b4fde2ac4 3ff08c692f75e5f3 3fff266bab3775b8 0 0 0
-35 +39,1,6 +4,0,4 | 3fb99999a0000000 3fbae147b4cccccd 0
-15 -38 +36,2,11 +24,1,4 +32,1,3 -18 +29,1,12 +23,0,15 +23,1,7 -19 +33,2,7 +27,1,16 +17,0,3 +25,1,12 -1 +35,1,9 +19,2,15 +12,2,13 +5,1,1 +21,2,5 +16,2,2 +35,0,2 +25,0,4 -38 -20 | 3fb99999a0000000 3fb99999a0000000 3fb3333338000000 3fb051eb8747ae14 3fbbd70a455c28f6 3fbbd70a455c28f6 3fc2b851f0ae147b 0 3fc23d70a8e147ae 3fc23d70a8e147ae 3fc153f7d3a2d0e5 3fc14e219b1413a9 3fee48bacf632265 3ff321ce0c87f79a 3ff321ce0c87f79a 3ff2558451f6afec 3fe60038625b3981 3fd60038625b3981 3fd7ea82ef65ab60 3fd0bdc20931ddc7 3fd7700fa82596b0 3fe327188ba0c5d5 3fdab6c3f957646c 3fdab6c3f957646c 3fdab6c3f957646c
+20,1,8 | 3fbc28f5c999999a
-39 -17 +10,1,9 -32 -22 -19 +29,2,14 -38 +25,2,7 -15 +9,1,11 +25,0,10 -34 -16 -27 +29,0,11 -27 -34 +5,0,12 +34,2,7 +38,0,15 +18,2,3 +26,2,15 +28,0,13 +24,1,7 +3,0,11 +38,2,10 -35 +20,1,14 +1,0,13 -33 +21,2,13 +27,1,7 +31,1,15 +30,2,4 +11,1,0 +33,2,2 -13 +14,2,10 | 3fb99999a0000000 3fb99999a0000000 3fb70a3d76666666 3fb70a3d76666666 3fb70a3d76666666 3fb70a3d76666666 3fc70a3d76666666 3fc70a3d76666666 3fc5e353fd47ae14 3fc5e353fd47ae14 3fbf9db234f5c28e 3fdd1eb854000000 3fdd1eb854000000 3fdd1eb854000000 3fdd1eb854000000 3fb0a3d70e666666 3fb0a3d70e666666 3fb0a3d70e666666 3fd8f5c290666666 3fd7b645a2c7ae14 3fbf9db231020c49 3fc94af4f660aa64 3fbe59f2c140cc78 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
+21,1,14 +1,0,15 +29,0,7 -36 +37,1,12 +34,0,3 -14 -0 | 3fc99999a0000000 0 0 0 0 3fe400000e000000 3fe400000e000000 3fe400000e000000
+25,2,15 +29,1,11 +2,0,16 -27 +22,2,16 -15 +1,1,3 +8,0,6 +29,0,11 -35 +15,2,10 +17,2,4 +15,1,13 +38,1,16 -37 +24,1,10 +13,1,7 +18,2,14 +2,1,10 +3,2,3 +21,2,6 -23 +9,0,17 -15 -25 -0 +34,1,7 +35,1,9 -25 -19 -35 +37,2,8 -0 +23,2,10 +20,1,13 -2 +6,1,14 +22,2,0 | 3faeb851f3333333 3fa70a3d76666666 3fa676c8b94ccccd 3fa676c8b94ccccd 3fa6686838c522a7 3fa6686838c522a7 3fb42ac435557668 3fbe8256c4805f8d 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
+0,1,5 +26,0,14 +7,2,4 +9,0,12 +35,1,2 +24,0,11 +32,1,15 +23,0,0 +12,2,15 -19 -19 -15 +26,2,9 +30,1,14 +4,2,9 +16,2,3 +5,1,6 +19,1,6 +28,2,11 +11,0,4 +25,0,14 +29,0,14 -8 +20,2,0 +39,1,2 +1,2,5 -0 +29,1,17 -36 +11,1,2 +20,0,2 +18,1,13 +31,0,8 +10,0,11 +7,0,15 | 3fb1eb851e147ae0 3fe8a3d703c28f5c 3fe4f1a9f3ee978e 3fee76c8a86978d6 3ff7ef9dad8ed916 3ff4322d0a7f7cee 3fe9b43951d81062 3fef6a7ef5f4bc69 3fe2d97f605fa43f 3fe2d97f605fa43f 3fe2d97f605fa43f 3fe2d97f605fa43f 3fcabb6ed8c193b2 3fe03af104f7414a 3fdd36e508f04252 3fe75f1da2f11fd6 3fe80f16fa9927e8 3fe8bf1052412ffa 3fe28f4c3db0e3fc 3fdc8db05c29e668 3ff56a44418db6c3 4001d88e360879f5 4001d88e360879f5 4007331f49cfee30 400c5aed5bca4cc0 4003d93fba950641 40068e2584600144 3ffb10f9d2f3c595 3ffb10f9d2f3c595 400144afc6e259a8 400080e60b44c8b6 3ffb50ef85ffbb4d 3ffca60df03a076d 3ff95141e6a84920 3ff783be6986e81a
+13,2,0 +17,0,15 +7,1,1 +35,0,5 -30 -25 -21 +37,2,5 +4,0,17 +37,0,12 -3 +10,2,11 +33,0,11 +26,1,4 -24 +3,2,2 +31,2,13 -36 | 3fc0a3d710f5c290 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
";

    fn value_pool_id(i: usize) -> String {
        const REAL: [&str; 5] = [
            "minecraft:sprinting",
            "minecraft:effect.speed",
            "minecraft:effect.slowness",
            "minecraft:powder_snow",
            "minecraft:base_speed",
        ];
        if i < REAL.len() {
            REAL[i].to_string()
        } else if i.is_multiple_of(3) {
            format!("mod:m{i}")
        } else {
            format!("minecraft:m{i}")
        }
    }

    /// The amounts the cases draw from: mostly `float` literals widened to `double`, as the game's.
    fn value_amounts() -> [f64; 18] {
        [
            f64::from(0.3_f32),
            f64::from(0.2_f32),
            f64::from(0.2_f32) * 2.0,
            f64::from(0.2_f32) * 3.0,
            f64::from(-0.15_f32),
            f64::from(-0.15_f32) * 2.0,
            0.05,
            -0.05,
            0.1,
            -0.1,
            0.25,
            -0.25,
            0.5,
            -0.5,
            1.0,
            -0.4,
            f64::from(-0.05_f32 * (7.0_f32 / 140.0_f32)),
            0.0,
        ]
    }

    #[test]
    fn attribute_values_match_attribute_instance() {
        let amounts = value_amounts();
        let ops = [
            Operation::AddValue,
            Operation::AddMultipliedBase,
            Operation::AddMultipliedTotal,
        ];
        let mut steps = 0;
        let mut collisions = 0;
        for line in VALUE_CASES.lines() {
            let (steps_text, values) = line.split_once('|').expect("case format");
            let mut a = Attributes::player();
            let ms = Attribute::MovementSpeed;
            for (op, want) in steps_text.split_whitespace().zip(values.split_whitespace()) {
                if let Some(id) = op.strip_prefix('-') {
                    a.remove_modifier(ms, &value_pool_id(id.parse().unwrap()));
                } else {
                    let mut parts = op[1..].split(',');
                    let id: usize = parts.next().unwrap().parse().unwrap();
                    let operation = ops[parts.next().unwrap().parse::<usize>().unwrap()];
                    let amount = amounts[parts.next().unwrap().parse::<usize>().unwrap()];
                    a.add_modifier(ms, m(&value_pool_id(id), amount, operation));
                }
                let want = u64::from_str_radix(want, 16).unwrap();
                assert_eq!(a.value(ms).to_bits(), want, "step {steps}: {line}");
                steps += 1;
            }
            // Count cases where two applied modifiers of one operation share a start slot.
            let inst = a.instance(ms);
            for t in &inst.tables {
                let mut starts = std::collections::HashSet::new();
                for s in t.slots.iter().filter(|&&s| s != 0) {
                    let i = usize::from(*s) - 1;
                    if !starts.insert((mix(inst.hashes[i]) as u32 as usize) & (t.slots.len() - 1)) {
                        collisions += 1;
                    }
                }
            }
        }
        assert!(steps > 1000, "{steps} steps");
        assert!(
            collisions > 5,
            "only {collisions} cases ended with colliding modifiers"
        );
    }
}
