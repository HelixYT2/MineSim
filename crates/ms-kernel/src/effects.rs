//! Status effects (`MobEffectInstance`): which are active, at what amplifier, for how long.
//! Movement-relevant effects act either through attribute modifiers (speed, slowness, jump boost's
//! safe-fall-distance, health boost) or are read directly by the physics (jump boost's jump power,
//! slow falling, levitation, dolphin's grace).
//!
//! # Two views of the same state
//!
//! The reference splits effect handling between the server and the client, and the recorded corpus
//! is the *client's* view, so both are modelled.
//!
//! * The **server view** ([`add_effect`], [`force_add_effect`], [`remove_effect`],
//!   [`clear_effects`], [`tick_effects`]) is `LivingEntity` run with `isClientSide() == false`:
//!   adding an effect applies its attribute modifiers at once, durations count down and an effect
//!   is dropped, modifiers and all, the tick its duration reaches zero. A simulator that is its own
//!   server (a potion drunk by the agent, an effect expiring) uses these.
//!
//! * The **client view** (`client_*`) is what a `LocalPlayer` does with the packets it receives.
//!   The client never touches attribute modifiers when an effect is added, updated or removed
//!   (every `onEffect*` hook is guarded by `!level().isClientSide()`); effect packets change the
//!   effect list only, and the modifiers arrive separately in `ClientboundUpdateAttributesPacket`,
//!   which replaces the modifiers of each attribute it names
//!   ([`crate::attributes::Attributes::apply_snapshot`]). The two packets are sent at different
//!   points of the server tick, so the attribute value can lag the effect list by a client tick
//!   (the corpus shows it: `effect_speed` row 151/152, `legacy_capture` row 11/12). The client also
//!   only counts durations down ([`client_tick_effects`]); it never expires an effect, the server's
//!   remove packet does.
//!
//! Sprinting is client-local: `LocalPlayer` calls `setSprinting`, which adds the sprint modifier
//! itself ([`set_sprinting`]); a later attribute snapshot from the server replaces it with the
//! server's own copy.
//!
//! Only the attributes in [`Attribute::ALL`] are modelled, so effects whose modifiers act on other
//! attributes (haste, mining fatigue, strength, weakness, luck, absorption's max-absorption,
//! invisibility's waypoint range) are tracked as plain effects without modifiers. Effects with
//! per-tick gameplay (regeneration, poison, wither, hunger, saturation, instant health/damage) are
//! tracked and counted down, but their health/food effects are not applied here.

use crate::attributes::{canonical_id, Attribute, Modifier, Operation, SPRINTING_MODIFIER_ID};
use crate::state::PlayerState;

pub const SPEED: &str = "minecraft:speed";
pub const SLOWNESS: &str = "minecraft:slowness";
pub const JUMP_BOOST: &str = "minecraft:jump_boost";
pub const HEALTH_BOOST: &str = "minecraft:health_boost";
pub const SLOW_FALLING: &str = "minecraft:slow_falling";
pub const LEVITATION: &str = "minecraft:levitation";
pub const DOLPHINS_GRACE: &str = "minecraft:dolphins_grace";
pub const FIRE_RESISTANCE: &str = "minecraft:fire_resistance";
pub const WATER_BREATHING: &str = "minecraft:water_breathing";
pub const RESISTANCE: &str = "minecraft:resistance";

/// `MobEffectInstance.INFINITE_DURATION`.
pub const INFINITE_DURATION: i32 = -1;

/// One active effect.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectInstance {
    /// Registry id, e.g. `"minecraft:speed"`.
    pub id: String,
    pub amplifier: i32,
    /// Remaining ticks (`-1` = infinite).
    pub duration: i32,
}

/// An effect that a stronger-but-shorter (or weaker-but-longer) one is covering
/// (`MobEffectInstance.hiddenEffect`), which takes over when the covering one runs out.
#[derive(Clone, Debug, PartialEq)]
struct Hidden {
    amplifier: i32,
    duration: i32,
    below: Option<Box<Hidden>>,
}

fn is_infinite(duration: i32) -> bool {
    duration == INFINITE_DURATION
}

/// `a.isShorterDurationThan(b)`.
fn shorter_than(a: i32, b: i32) -> bool {
    !is_infinite(a) && (a < b || is_infinite(b))
}

/// `mapDuration(i -> i - 1)`: infinite and zero durations stay as they are.
fn counted_down(duration: i32) -> i32 {
    if is_infinite(duration) || duration == 0 {
        duration
    } else {
        duration.wrapping_sub(1)
    }
}

/// `hasRemainingDuration`.
fn has_remaining(duration: i32) -> bool {
    is_infinite(duration) || duration > 0
}

impl Hidden {
    /// `MobEffectInstance.update` applied to a hidden layer; the result is not needed.
    fn update(&mut self, amplifier: i32, duration: i32) {
        if amplifier > self.amplifier {
            if shorter_than(duration, self.duration) {
                let below = self.below.take();
                self.below = Some(Box::new(Hidden {
                    amplifier: self.amplifier,
                    duration: self.duration,
                    below,
                }));
            }
            self.amplifier = amplifier;
            self.duration = duration;
        } else if shorter_than(self.duration, duration) {
            if amplifier == self.amplifier {
                self.duration = duration;
            } else {
                match &mut self.below {
                    Some(below) => below.update(amplifier, duration),
                    None => {
                        self.below = Some(Box::new(Hidden {
                            amplifier,
                            duration,
                            below: None,
                        }));
                    }
                }
            }
        }
    }

    fn tick_down(&mut self) {
        if let Some(below) = &mut self.below {
            below.tick_down();
        }
        self.duration = counted_down(self.duration);
    }
}

/// The set of active effects (`LivingEntity.activeEffects`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    list: Vec<EffectInstance>,
    /// `hiddenEffect` of each entry of `list`, parallel to it.
    hidden: Vec<Option<Box<Hidden>>>,
}

impl Effects {
    /// The active effect with this registry id (`minecraft:` may be left out).
    pub fn get(&self, id: &str) -> Option<&EffectInstance> {
        let id = canonical_id(id);
        self.list.iter().find(|e| e.id == id)
    }

    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// The amplifier of the active effect with this id.
    pub fn amplifier(&self, id: &str) -> Option<i32> {
        self.get(id).map(|e| e.amplifier)
    }

    pub fn iter(&self) -> impl Iterator<Item = &EffectInstance> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Insert or replace the effect with this id (no attribute bookkeeping, and any covered hidden
    /// effect is forgotten; see the module docs for the attribute-aware functions).
    pub fn insert(&mut self, mut e: EffectInstance) {
        e.id = canonical_id(&e.id).into_owned();
        match self.list.iter().position(|x| x.id == e.id) {
            Some(i) => {
                self.list[i] = e;
                self.hidden[i] = None;
            }
            None => {
                self.list.push(e);
                self.hidden.push(None);
            }
        }
    }

    /// Remove the effect with this id (no attribute bookkeeping).
    pub fn remove(&mut self, id: &str) -> Option<EffectInstance> {
        let id = canonical_id(id);
        let i = self.list.iter().position(|e| e.id == id)?;
        self.hidden.remove(i);
        Some(self.list.remove(i))
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.hidden.clear();
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut EffectInstance> {
        self.list.iter_mut()
    }

    /// `MobEffectInstance.update` of the entry at `i` with an incoming effect; true if the entry's
    /// own amplifier or duration changed (the game's "changed" result, minus the particle and icon
    /// flags this model does not carry).
    fn update_entry(&mut self, i: usize, amplifier: i32, duration: i32) -> bool {
        let e = &mut self.list[i];
        let mut changed = false;
        if amplifier > e.amplifier {
            if shorter_than(duration, e.duration) {
                let below = self.hidden[i].take();
                self.hidden[i] = Some(Box::new(Hidden {
                    amplifier: e.amplifier,
                    duration: e.duration,
                    below,
                }));
            }
            e.amplifier = amplifier;
            e.duration = duration;
            changed = true;
        } else if shorter_than(e.duration, duration) {
            if amplifier == e.amplifier {
                e.duration = duration;
                changed = true;
            } else {
                match &mut self.hidden[i] {
                    Some(h) => h.update(amplifier, duration),
                    None => {
                        self.hidden[i] = Some(Box::new(Hidden {
                            amplifier,
                            duration,
                            below: None,
                        }));
                    }
                }
            }
        }
        changed
    }

    /// `tickDownDuration` (including covered effects) followed by `downgradeToHiddenEffect`;
    /// returns whether the entry was downgraded to the effect it covered.
    fn count_down_entry(&mut self, i: usize) -> bool {
        if let Some(h) = &mut self.hidden[i] {
            h.tick_down();
        }
        let e = &mut self.list[i];
        e.duration = counted_down(e.duration);
        if e.duration == 0 {
            if let Some(h) = self.hidden[i].take() {
                e.amplifier = h.amplifier;
                e.duration = h.duration;
                self.hidden[i] = h.below;
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------
// Effect -> attribute modifiers (MobEffects)
// ---------------------------------------------------------------------------------------------

/// `MobEffect.AttributeTemplate`: the modifier an effect applies to one attribute, scaled by
/// `amplifier + 1`.
struct Template {
    attribute: Attribute,
    id: &'static str,
    amount: f64,
    operation: Operation,
}

impl Template {
    /// `AttributeTemplate.create(amplifier)`: `amount * (amplifier + 1)` with the `int` sum widened
    /// to `double`.
    fn create(&self, amplifier: i32) -> Modifier {
        Modifier {
            id: self.id.to_string(),
            amount: self.amount * f64::from(amplifier.wrapping_add(1)),
            operation: self.operation,
        }
    }
}

// The `0.2F` / `-0.15F` literals in `MobEffects` are floats widened to double, hence the casts.
static SPEED_MODIFIERS: [Template; 1] = [Template {
    attribute: Attribute::MovementSpeed,
    id: "minecraft:effect.speed",
    amount: 0.2_f32 as f64,
    operation: Operation::AddMultipliedTotal,
}];
static SLOWNESS_MODIFIERS: [Template; 1] = [Template {
    attribute: Attribute::MovementSpeed,
    id: "minecraft:effect.slowness",
    amount: -0.15_f32 as f64,
    operation: Operation::AddMultipliedTotal,
}];
static JUMP_BOOST_MODIFIERS: [Template; 1] = [Template {
    attribute: Attribute::SafeFallDistance,
    id: "minecraft:effect.jump_boost",
    amount: 1.0,
    operation: Operation::AddValue,
}];
static HEALTH_BOOST_MODIFIERS: [Template; 1] = [Template {
    attribute: Attribute::MaxHealth,
    id: "minecraft:effect.health_boost",
    amount: 4.0,
    operation: Operation::AddValue,
}];

/// The attribute modifiers of an effect, for the attributes this crate models.
fn templates(effect: &str) -> &'static [Template] {
    match effect {
        SPEED => &SPEED_MODIFIERS,
        SLOWNESS => &SLOWNESS_MODIFIERS,
        JUMP_BOOST => &JUMP_BOOST_MODIFIERS,
        HEALTH_BOOST => &HEALTH_BOOST_MODIFIERS,
        _ => &[],
    }
}

/// Every effect that carries attribute modifiers here.
const MODIFIER_EFFECTS: [&str; 4] = [SPEED, SLOWNESS, JUMP_BOOST, HEALTH_BOOST];

/// The attributes some effect modifies, in the order the client resynchronises them.
const EFFECT_ATTRIBUTES: [Attribute; 3] = [
    Attribute::MovementSpeed,
    Attribute::SafeFallDistance,
    Attribute::MaxHealth,
];

/// `MobEffect.addAttributeModifiers`: for each of the effect's modifiers, drop one with the same
/// id and apply the amplified one.
fn add_effect_modifiers(p: &mut PlayerState, effect: &str, amplifier: i32) {
    for t in templates(effect) {
        p.attributes.add_modifier(t.attribute, t.create(amplifier));
    }
}

/// `MobEffect.removeAttributeModifiers`.
fn remove_effect_modifiers(p: &mut PlayerState, effect: &str) {
    for t in templates(effect) {
        p.attributes.remove_modifier(t.attribute, t.id);
    }
}

/// `onEffectUpdated(.., true)`: replace the effect's modifiers with ones for its current amplifier.
fn refresh_effect_modifiers(p: &mut PlayerState, effect: &str, amplifier: i32) {
    remove_effect_modifiers(p, effect);
    add_effect_modifiers(p, effect, amplifier);
}

/// `LivingEntity.onAttributeUpdated` for `max_health`: a health above the new maximum is lowered
/// to it (`setHealth` clamps to `[0, maxHealth]`; only the upper bound can newly apply).
fn cap_health(p: &mut PlayerState) {
    let max = p.attributes.value(Attribute::MaxHealth) as f32;
    if p.health > max {
        p.health = max;
    }
}

// ---------------------------------------------------------------------------------------------
// Server view
// ---------------------------------------------------------------------------------------------

/// `LivingEntity.tickEffects` as the server runs it (the simulator is its own server): count every
/// effect's duration down; an effect whose duration reaches zero is removed with its attribute
/// modifiers, and one that uncovers a hidden effect takes its amplifier and duration.
///
/// The real client keeps an expired effect at duration 0 until the server's remove packet arrives
/// (see [`client_tick_effects`]); that delay depends on the thread timing of an integrated server
/// and cannot be reproduced, so expiry takes effect at the tick it happens on the server.
pub fn tick_effects(p: &mut PlayerState) {
    let mut i = 0;
    while i < p.effects.list.len() {
        // MobEffectInstance.tickServer: an effect with nothing left is removed; otherwise count down.
        let alive = has_remaining(p.effects.list[i].duration) && {
            let downgraded = p.effects.count_down_entry(i);
            if downgraded {
                let (id, amp) = {
                    let e = &p.effects.list[i];
                    (e.id.clone(), e.amplifier)
                };
                refresh_effect_modifiers(p, &id, amp);
                cap_health(p);
            }
            has_remaining(p.effects.list[i].duration)
        };
        if alive {
            i += 1;
        } else {
            let removed = p.effects.list.remove(i);
            p.effects.hidden.remove(i);
            remove_effect_modifiers(p, &removed.id);
            cap_health(p);
        }
    }
}

/// `LivingEntity.addEffect`: a new effect is applied with its modifiers; an effect already active
/// is updated by the rules of `MobEffectInstance.update` (a higher amplifier replaces it, an equal
/// amplifier with a longer duration extends it, anything else is kept as a covered effect or
/// ignored) and its modifiers are refreshed if it changed. Returns whether anything changed.
///
/// The amplifier is clamped to `0..=255` as `MobEffectInstance`'s constructor does.
pub fn add_effect(p: &mut PlayerState, id: &str, amplifier: i32, duration: i32) -> bool {
    let id = &*canonical_id(id);
    let amplifier = amplifier.clamp(0, 255);
    match p.effects.list.iter().position(|e| e.id == id) {
        None => {
            p.effects.insert(EffectInstance {
                id: id.to_string(),
                amplifier,
                duration,
            });
            add_effect_modifiers(p, id, amplifier);
            cap_health(p);
            true
        }
        Some(i) => {
            let changed = p.effects.update_entry(i, amplifier, duration);
            if changed {
                let amp = p.effects.list[i].amplifier;
                refresh_effect_modifiers(p, id, amp);
                cap_health(p);
            }
            changed
        }
    }
}

/// `LivingEntity.forceAddEffect`: install the effect unconditionally, replacing any active effect
/// of the same kind (and whatever it covered), and (re)apply its modifiers.
pub fn force_add_effect(p: &mut PlayerState, id: &str, amplifier: i32, duration: i32) {
    let id = &*canonical_id(id);
    let amplifier = amplifier.clamp(0, 255);
    let existed = p.effects.has(id);
    p.effects.insert(EffectInstance {
        id: id.to_string(),
        amplifier,
        duration,
    });
    if existed {
        refresh_effect_modifiers(p, id, amplifier);
    } else {
        add_effect_modifiers(p, id, amplifier);
    }
    cap_health(p);
}

/// `LivingEntity.removeEffect`: remove one effect and its attribute modifiers; returns whether it
/// was active.
pub fn remove_effect(p: &mut PlayerState, id: &str) -> bool {
    let id = &*canonical_id(id);
    if p.effects.remove(id).is_some() {
        remove_effect_modifiers(p, id);
        cap_health(p);
        true
    } else {
        false
    }
}

/// `LivingEntity.removeAllEffects`: remove every effect and their attribute modifiers; returns
/// whether there were any.
pub fn clear_effects(p: &mut PlayerState) -> bool {
    if p.effects.is_empty() {
        return false;
    }
    let all: Vec<EffectInstance> = p.effects.iter().cloned().collect();
    p.effects.clear();
    for e in &all {
        remove_effect_modifiers(p, &e.id);
    }
    cap_health(p);
    true
}

/// `LivingEntity.setSprinting`: the flag plus the sprint speed modifier (`+0.3F`, a float widened
/// to double, `ADD_MULTIPLIED_TOTAL`), removed first and re-added when sprinting.
pub fn set_sprinting(p: &mut PlayerState, sprinting: bool) {
    p.sprinting = sprinting;
    p.attributes
        .remove_modifier(Attribute::MovementSpeed, SPRINTING_MODIFIER_ID);
    if sprinting {
        p.attributes
            .add_modifier(Attribute::MovementSpeed, Modifier::sprinting());
    }
}

// ---------------------------------------------------------------------------------------------
// Powder-snow slowdown
// ---------------------------------------------------------------------------------------------

/// The identifier of the freezing speed modifier (`LivingEntity.SPEED_MODIFIER_POWDER_SNOW_ID`).
pub const POWDER_SNOW_MODIFIER_ID: &str = "minecraft:powder_snow";

/// `Entity.getTicksRequiredToFreeze`.
const TICKS_REQUIRED_TO_FREEZE: i32 = 140;

/// `Entity.getPercentFrozen`: `(float)Math.min(ticksFrozen, 140) / 140`, in float arithmetic.
fn percent_frozen(ticks_frozen: i32) -> f32 {
    ticks_frozen.min(TICKS_REQUIRED_TO_FREEZE) as f32 / TICKS_REQUIRED_TO_FREEZE as f32
}

/// The modifier `LivingEntity.tryAddFrost` applies for `ticks_frozen` (> 0): `ADD_VALUE` of
/// `-0.05F * percentFrozen`, computed in `float` and widened.
pub fn frost_modifier(ticks_frozen: i32) -> Modifier {
    Modifier {
        id: POWDER_SNOW_MODIFIER_ID.to_string(),
        amount: f64::from(-0.05_f32 * percent_frozen(ticks_frozen)),
        operation: Operation::AddValue,
    }
}

/// `LivingEntity.removeFrost`. Part of the *server's* `aiStep` (`ServerLevel` branch): the client
/// only sees the result through attribute-update packets, whose timing follows the server's tick
/// rather than the client's, so a client replay cannot derive it from `ticks_frozen` alone.
pub fn remove_frost(p: &mut PlayerState) {
    p.attributes
        .remove_modifier(Attribute::MovementSpeed, POWDER_SNOW_MODIFIER_ID);
}

/// `LivingEntity.tryAddFrost`: while frozen (`ticks_frozen > 0`) and standing on something that is
/// not air (`getBlockStateOnLegacy().isAir()` false), slow the movement speed.
pub fn try_add_frost(p: &mut PlayerState, standing_on_air: bool) {
    if !standing_on_air && p.ticks_frozen > 0 {
        let modifier = frost_modifier(p.ticks_frozen);
        p.attributes
            .add_modifier(Attribute::MovementSpeed, modifier);
    }
}

// ---------------------------------------------------------------------------------------------
// Client view
// ---------------------------------------------------------------------------------------------

/// `MobEffectInstance.tickClient` for every effect: an effect with time left counts down by one
/// (infinite effects stay infinite), and never goes below zero; nothing is ever removed and no
/// modifier changes, because on the client only the server's packets do that.
pub fn client_tick_effects(p: &mut PlayerState) {
    for i in 0..p.effects.list.len() {
        if has_remaining(p.effects.list[i].duration) {
            p.effects.count_down_entry(i);
        }
    }
}

/// `ClientPacketListener.handleUpdateMobEffect`: the client installs the effect with
/// `forceAddEffect`, which on the client only replaces the list entry (a fresh instance, covering
/// nothing). Attribute modifiers are untouched; see [`client_sync_attributes`].
pub fn client_update_mob_effect(p: &mut PlayerState, id: &str, amplifier: i32, duration: i32) {
    p.effects.insert(EffectInstance {
        id: id.to_string(),
        amplifier: amplifier.clamp(0, 255),
        duration,
    });
}

/// `ClientPacketListener.handleRemoveMobEffect` (`removeEffectNoUpdate`): drop the list entry only.
pub fn client_remove_mob_effect(p: &mut PlayerState, id: &str) {
    p.effects.remove(id);
}

/// Bring the client's effect list in line with the server's by the packets that would do it: a
/// remove packet for each effect the server no longer has, an update packet for each new or
/// changed one. Entries that already match are left alone (a packet would reset the same values).
pub fn client_sync_effects(p: &mut PlayerState, server: &[EffectInstance]) {
    let wanted: Vec<EffectInstance> = server
        .iter()
        .map(|s| EffectInstance {
            id: canonical_id(&s.id).into_owned(),
            amplifier: s.amplifier.clamp(0, 255),
            duration: s.duration,
        })
        .collect();
    let stale: Vec<String> = p
        .effects
        .iter()
        .filter(|e| !wanted.iter().any(|w| w.id == e.id))
        .map(|e| e.id.clone())
        .collect();
    for id in stale {
        client_remove_mob_effect(p, &id);
    }
    for w in wanted {
        if p.effects.get(&w.id) != Some(&w) {
            client_update_mob_effect(p, &w.id, w.amplifier, w.duration);
        }
    }
}

/// Deliver the attribute-update packets (`ClientboundUpdateAttributesPacket`) the server sends after
/// effect changes, for every attribute an effect can modify. See [`client_sync_attributes_for`].
pub fn client_sync_attributes(p: &mut PlayerState) {
    client_sync_attributes_for(p, &EFFECT_ATTRIBUTES);
}

/// Deliver the attribute-update packet for each listed attribute (those no effect can modify are
/// skipped: there is nothing to derive for them).
///
/// The server's modifiers on such an attribute are the effects' modifiers for the effects in the
/// list plus whatever else it applied (the sprint modifier follows `p.sprinting`, assuming the
/// server has already seen the client's sprint state; other non-effect modifiers already on the
/// client, such as the freeze slowdown, are carried over). The client replaces its own modifiers by
/// that list. Effect modifiers are listed first, in effect order, then the others in their existing
/// order, then sprinting; the order only matters for modifiers whose identifiers collide in the
/// game's hash table, which none of the ones modelled do.
pub fn client_sync_attributes_for(p: &mut PlayerState, attributes: &[Attribute]) {
    for &attribute in attributes {
        if !EFFECT_ATTRIBUTES.contains(&attribute) {
            continue;
        }
        let (base, current) = p.attributes.snapshot(attribute);
        let is_effect_modifier = |m: &Modifier| {
            MODIFIER_EFFECTS
                .iter()
                .flat_map(|e| templates(e))
                .any(|t| t.id == m.id)
        };
        let mut list: Vec<Modifier> = Vec::new();
        for e in p.effects.iter() {
            for t in templates(&e.id) {
                if t.attribute == attribute {
                    list.push(t.create(e.amplifier));
                }
            }
        }
        list.extend(
            current
                .into_iter()
                .filter(|m| !is_effect_modifier(m) && m.id != SPRINTING_MODIFIER_ID),
        );
        if attribute == Attribute::MovementSpeed && p.sprinting {
            list.push(Modifier::sprinting());
        }
        p.attributes.apply_snapshot(attribute, base, &list);
    }
}

/// What the server changed between two client ticks, as far as effects and attributes go, applied in
/// the order the client sees the packets: the effect list, then the sprint flag, then the attribute
/// updates. This is what a replay driver calls for the recorded `pre` diff of a tick (in place of
/// writing the `effects` and `sprinting` fields directly, which would leave the attribute modifiers
/// out of step):
///
/// * `effects`: the server's complete effect list, if it changed;
/// * `sprinting`: the sprint flag, if the server changed it (the client's own sprint changes go
///   through [`set_sprinting`] inside its tick);
/// * `attributes_updated`: the attributes an attribute-update packet arrived for (the ones whose
///   recorded value changed; all of them for the first recorded tick).
pub fn client_apply_server_changes(
    p: &mut PlayerState,
    effects: Option<&[EffectInstance]>,
    sprinting: Option<bool>,
    attributes_updated: &[Attribute],
) {
    if let Some(list) = effects {
        client_sync_effects(p, list);
    }
    if let Some(s) = sprinting {
        if s != p.sprinting {
            set_sprinting(p, s);
        }
    }
    client_sync_attributes_for(p, attributes_updated);
}

// ---------------------------------------------------------------------------------------------
// Queries the physics needs
// ---------------------------------------------------------------------------------------------

/// `LivingEntity.hasEffect`.
pub fn has_effect(p: &PlayerState, id: &str) -> bool {
    p.effects.has(id)
}

/// `getEffect(id).getAmplifier()` if the effect is active.
pub fn effect_amplifier(p: &PlayerState, id: &str) -> Option<i32> {
    p.effects.amplifier(id)
}

/// `LivingEntity.getJumpBoostPower`: `0.1F * (amplifier + 1.0F)` in `float` arithmetic, 0 without
/// the effect.
pub fn jump_boost_power(p: &PlayerState) -> f32 {
    match p.effects.amplifier(JUMP_BOOST) {
        Some(amp) => 0.1_f32 * (amp as f32 + 1.0_f32),
        None => 0.0,
    }
}

/// The levitation amplifier (`travelInAir` pulls the vertical velocity towards
/// `0.05 * (amplifier + 1)`), or `None` without the effect.
pub fn levitation_amplifier(p: &PlayerState) -> Option<i32> {
    p.effects.amplifier(LEVITATION)
}

pub fn has_levitation(p: &PlayerState) -> bool {
    p.effects.has(LEVITATION)
}

/// Slow falling lowers the gravity of a descending entity (`getEffectiveGravity`) and, like
/// levitation, resets the fall distance each tick.
pub fn has_slow_falling(p: &PlayerState) -> bool {
    p.effects.has(SLOW_FALLING)
}

/// `hasEffect(DOLPHINS_GRACE)`: the water drag factor becomes 0.96.
pub fn has_dolphins_grace(p: &PlayerState) -> bool {
    p.effects.has(DOLPHINS_GRACE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ms_numerics::Vec3;

    fn player() -> PlayerState {
        PlayerState::new(Vec3::ZERO, 0.0)
    }

    fn ms(p: &PlayerState) -> f64 {
        p.attributes.value(Attribute::MovementSpeed)
    }

    #[test]
    fn speed_effect_scales_with_amplifier() {
        let mut p = player();
        assert!(add_effect(&mut p, SPEED, 1, 100_000));
        // 0.1F * (1 + 0.2F * 2), as recorded by the oracle (effect_speed, row 0).
        assert_eq!(ms(&p).to_bits(), 0.140_000_002_682_209_02_f64.to_bits());
        let m = p
            .attributes
            .modifier(Attribute::MovementSpeed, "minecraft:effect.speed")
            .unwrap();
        assert_eq!(m.amount.to_bits(), (f64::from(0.2_f32) * 2.0).to_bits());
        assert!(remove_effect(&mut p, SPEED));
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
        assert!(!remove_effect(&mut p, SPEED));
    }

    #[test]
    fn slowness_and_sprinting_stack_in_table_order() {
        let mut p = player();
        add_effect(&mut p, SPEED, 1, 1200);
        set_sprinting(&mut p, true);
        assert_eq!(ms(&p).to_bits(), 0.182_000_005_155_801_8_f64.to_bits());
        add_effect(&mut p, SLOWNESS, 0, 600);
        assert_eq!(ms(&p).to_bits(), 0.154_700_003_297_626_98_f64.to_bits());
        set_sprinting(&mut p, false);
        assert_eq!(ms(&p).to_bits(), 0.119_000_001_445_412_63_f64.to_bits());
        assert!(clear_effects(&mut p));
        assert!(!clear_effects(&mut p));
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
        assert!(p.attributes.modifiers(Attribute::MovementSpeed).is_empty());
    }

    #[test]
    fn set_sprinting_adds_and_removes_the_modifier() {
        let mut p = player();
        set_sprinting(&mut p, true);
        assert!(p.sprinting);
        let m = p
            .attributes
            .modifier(Attribute::MovementSpeed, SPRINTING_MODIFIER_ID)
            .unwrap();
        // 0.3F widened to double, not 0.3.
        assert_eq!(m.amount.to_bits(), 0.300_000_011_920_928_96_f64.to_bits());
        assert_eq!(m.operation, Operation::AddMultipliedTotal);
        assert_eq!(ms(&p).to_bits(), 0.130_000_003_129_243_87_f64.to_bits());
        // Setting it twice does not stack.
        set_sprinting(&mut p, true);
        assert_eq!(p.attributes.modifiers(Attribute::MovementSpeed).len(), 1);
        set_sprinting(&mut p, false);
        assert!(!p.sprinting);
        assert!(p.attributes.modifiers(Attribute::MovementSpeed).is_empty());
    }

    #[test]
    fn jump_boost_changes_safe_fall_distance_and_jump_power() {
        let mut p = player();
        assert_eq!(jump_boost_power(&p), 0.0);
        add_effect(&mut p, JUMP_BOOST, 1, 100_000);
        assert_eq!(p.attributes.value(Attribute::SafeFallDistance), 5.0);
        assert_eq!(jump_boost_power(&p).to_bits(), (0.1_f32 * 2.0).to_bits());
        // Upgrading to amplifier 4 (oracle: effect_jump_boost row 106).
        assert!(add_effect(&mut p, JUMP_BOOST, 4, 100_000));
        assert_eq!(p.attributes.value(Attribute::SafeFallDistance), 8.0);
        assert_eq!(jump_boost_power(&p).to_bits(), (0.1_f32 * 5.0).to_bits());
        remove_effect(&mut p, JUMP_BOOST);
        assert_eq!(p.attributes.value(Attribute::SafeFallDistance), 3.0);
    }

    #[test]
    fn add_effect_update_rules() {
        let mut p = player();
        assert!(add_effect(&mut p, SPEED, 1, 100));
        // Weaker and shorter: ignored.
        assert!(!add_effect(&mut p, SPEED, 0, 50));
        assert_eq!(p.effects.get(SPEED).unwrap().amplifier, 1);
        assert_eq!(p.effects.get(SPEED).unwrap().duration, 100);
        // Same amplifier, longer: extends.
        assert!(add_effect(&mut p, SPEED, 1, 300));
        assert_eq!(p.effects.get(SPEED).unwrap().duration, 300);
        // Same amplifier, shorter: ignored.
        assert!(!add_effect(&mut p, SPEED, 1, 200));
        assert_eq!(p.effects.get(SPEED).unwrap().duration, 300);
        // Stronger and shorter: replaces; the old one is covered and returns later.
        assert!(add_effect(&mut p, SPEED, 2, 40));
        assert_eq!(p.effects.get(SPEED).unwrap().amplifier, 2);
        assert_eq!(
            ms(&p).to_bits(),
            (f64::from(0.1_f32) * (1.0 + f64::from(0.2_f32) * 3.0)).to_bits()
        );
        for _ in 0..40 {
            tick_effects(&mut p);
        }
        // The covered amplifier-1 effect, counted down alongside, takes over with 300 - 40 left.
        let e = p.effects.get(SPEED).unwrap();
        assert_eq!((e.amplifier, e.duration), (1, 260));
        assert_eq!(
            ms(&p).to_bits(),
            (f64::from(0.1_f32) * (1.0 + f64::from(0.2_f32) * 2.0)).to_bits()
        );
        // The amplifier is clamped like MobEffectInstance's constructor.
        assert!(add_effect(&mut p, SLOWNESS, 1000, 10));
        assert_eq!(p.effects.get(SLOWNESS).unwrap().amplifier, 255);
        assert!(add_effect(&mut p, LEVITATION, -3, 10));
        assert_eq!(levitation_amplifier(&p), Some(0));
    }

    #[test]
    fn weaker_longer_effect_waits_behind_the_stronger_one() {
        let mut p = player();
        add_effect(&mut p, SPEED, 2, 20);
        // Weaker and longer: covered, not applied.
        assert!(!add_effect(&mut p, SPEED, 0, 100));
        assert_eq!(p.effects.get(SPEED).unwrap().amplifier, 2);
        for _ in 0..20 {
            tick_effects(&mut p);
        }
        let e = p.effects.get(SPEED).unwrap();
        assert_eq!((e.amplifier, e.duration), (0, 80));
        assert_eq!(
            ms(&p).to_bits(),
            (f64::from(0.1_f32) * (1.0 + f64::from(0.2_f32) * 1.0)).to_bits()
        );
    }

    #[test]
    fn force_add_replaces_unconditionally() {
        let mut p = player();
        add_effect(&mut p, SPEED, 3, 1000);
        add_effect(&mut p, SPEED, 0, 5000);
        force_add_effect(&mut p, SPEED, 0, 10);
        let e = p.effects.get(SPEED).unwrap();
        assert_eq!((e.amplifier, e.duration), (0, 10));
        assert_eq!(
            ms(&p).to_bits(),
            (f64::from(0.1_f32) * (1.0 + f64::from(0.2_f32))).to_bits()
        );
        // Nothing is covered any more: it just expires.
        for _ in 0..10 {
            tick_effects(&mut p);
        }
        assert!(p.effects.is_empty());
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
    }

    #[test]
    fn effects_expire_with_their_modifiers() {
        let mut p = player();
        add_effect(&mut p, SLOWNESS, 0, 3);
        add_effect(&mut p, FIRE_RESISTANCE, 0, INFINITE_DURATION);
        tick_effects(&mut p);
        assert_eq!(p.effects.get(SLOWNESS).unwrap().duration, 2);
        tick_effects(&mut p);
        tick_effects(&mut p);
        assert!(!p.effects.has(SLOWNESS));
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
        // Infinite effects do not count down.
        for _ in 0..5 {
            tick_effects(&mut p);
        }
        assert_eq!(
            p.effects.get(FIRE_RESISTANCE).unwrap().duration,
            INFINITE_DURATION
        );
    }

    #[test]
    fn client_never_expires_or_touches_modifiers() {
        let mut p = player();
        client_update_mob_effect(&mut p, SPEED, 1, 2);
        // The packet changes the list only.
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
        client_sync_attributes(&mut p);
        assert_eq!(ms(&p).to_bits(), 0.140_000_002_682_209_02_f64.to_bits());
        client_tick_effects(&mut p);
        client_tick_effects(&mut p);
        // Duration 0 is kept (and stays 0), with its modifier, until the remove packet.
        client_tick_effects(&mut p);
        assert_eq!(p.effects.get(SPEED).unwrap().duration, 0);
        assert_eq!(ms(&p).to_bits(), 0.140_000_002_682_209_02_f64.to_bits());
        client_remove_mob_effect(&mut p, SPEED);
        assert!(p.effects.is_empty());
        assert_eq!(ms(&p).to_bits(), 0.140_000_002_682_209_02_f64.to_bits());
        client_sync_attributes(&mut p);
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
    }

    #[test]
    fn client_snapshot_keeps_the_sprint_modifier_and_is_idempotent() {
        let mut p = player();
        set_sprinting(&mut p, true);
        client_sync_effects(
            &mut p,
            &[EffectInstance {
                id: SLOWNESS.to_string(),
                amplifier: 0,
                duration: 600,
            }],
        );
        client_sync_attributes(&mut p);
        let want = f64::from(0.1_f32) * (1.0 + f64::from(0.3_f32)) * (1.0 + f64::from(-0.15_f32));
        assert_eq!(ms(&p).to_bits(), want.to_bits());
        let before = p.clone();
        client_sync_attributes(&mut p);
        assert_eq!(p, before);
    }

    #[test]
    fn frost_slows_movement_speed_by_the_float_percentage() {
        let mut p = player();
        p.ticks_frozen = 56;
        try_add_frost(&mut p, false);
        // Oracle: ladder_climb row 0 (the player starts still frozen).
        assert_eq!(ms(&p).to_bits(), 0.080_000_000_074_505_8_f64.to_bits());
        p.ticks_frozen = 1;
        try_add_frost(&mut p, false);
        assert_eq!(ms(&p).to_bits(), 0.099_642_858_636_798_34_f64.to_bits());
        // Frozen but over air: no modifier.
        remove_frost(&mut p);
        try_add_frost(&mut p, true);
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
        // Past the full-freeze threshold the slowdown stops growing.
        p.ticks_frozen = 500;
        try_add_frost(&mut p, false);
        assert_eq!(
            ms(&p).to_bits(),
            (f64::from(0.1_f32) + f64::from(-0.05_f32)).to_bits()
        );
        remove_frost(&mut p);
        assert_eq!(ms(&p).to_bits(), f64::from(0.1_f32).to_bits());
    }

    #[test]
    fn health_boost_removal_lowers_health_to_the_new_maximum() {
        let mut p = player();
        add_effect(&mut p, HEALTH_BOOST, 1, 1000);
        assert_eq!(p.attributes.value(Attribute::MaxHealth), 28.0);
        p.health = 27.5;
        remove_effect(&mut p, HEALTH_BOOST);
        assert_eq!(p.attributes.value(Attribute::MaxHealth), 20.0);
        assert_eq!(p.health, 20.0);
    }

    #[test]
    fn queries() {
        let mut p = player();
        assert!(!has_slow_falling(&p) && !has_levitation(&p) && !has_dolphins_grace(&p));
        add_effect(&mut p, SLOW_FALLING, 0, 100);
        add_effect(&mut p, LEVITATION, 1, 100);
        add_effect(&mut p, DOLPHINS_GRACE, 0, 100);
        assert!(has_slow_falling(&p) && has_levitation(&p) && has_dolphins_grace(&p));
        assert_eq!(levitation_amplifier(&p), Some(1));
        assert_eq!(effect_amplifier(&p, SLOW_FALLING), Some(0));
        assert!(has_effect(&p, DOLPHINS_GRACE));
        assert!(!has_effect(&p, SPEED));
    }

    /// Cases generated with the real 1.21.11 `MobEffectInstance` (`update`, then the countdown and
    /// downgrade steps of `tickServer`): random "add speed amplifier,duration" (`uA,D`, duration -1 is
    /// infinite) and server-tick (`t`) sequences, and the state after each step: `!` / `=` for
    /// whether an add changed the effect, then `amplifier:duration` of the effect followed by each
    /// effect it covers (`<amplifier:duration`), or `X` once the effect has expired.
    const EFFECT_CASES: &str = "\
u2,8 t t u2,-1 | !2:8 2:7 2:6 !2:-1
u3,3 u3,8 t | !3:3 !3:8 3:7
u1,5 u1,4 u3,3 u0,-1 u2,3 u0,5 u2,4 | !1:5 =1:5 !3:3<1:5 =3:3<1:5<0:-1 =3:3<1:5<0:-1 =3:3<1:5<0:-1 =3:3<2:4<1:5<0:-1
u2,1 u3,6 t u3,7 | !2:1 !3:6 3:5 !3:7
u2,6 t t u0,1 t u2,7 | !2:6 2:5 2:4 =2:4 2:3 !2:7
u0,2 u0,8 t | !0:2 !0:8 0:7
u3,3 u1,9 u2,-1 u2,1 u3,2 t t u3,-1 u0,3 t t | !3:3 =3:3<1:9 =3:3<2:-1 =3:3<2:-1 =3:3<2:-1 3:2<2:-1 3:1<2:-1 !3:-1<2:-1 =3:-1<2:-1 3:-1<2:-1 3:-1<2:-1
u2,7 u0,6 u1,8 u0,9 u1,6 t t u3,3 u3,1 t u2,2 | !2:7 =2:7 =2:7<1:8 =2:7<1:8<0:9 =2:7<1:8<0:9 2:6<1:7<0:8 2:5<1:6<0:7 !3:3<2:5<1:6<0:7 =3:3<2:5<1:6<0:7 3:2<2:4<1:5<0:6 =3:2<2:4<1:5<0:6
u2,5 t u2,3 u3,2 | !2:5 2:4 =2:4 !3:2<2:4
u1,3 t u2,3 t u0,-1 u1,7 u1,5 u2,8 t t t t u3,1 u0,5 | !1:3 1:2 !2:3 2:2 =2:2<0:-1 =2:2<1:7<0:-1 =2:2<1:7<0:-1 !2:8<1:7<0:-1 2:7<1:6<0:-1 2:6<1:5<0:-1 2:5<1:4<0:-1 2:4<1:3<0:-1 !3:1<2:4<1:3<0:-1 =3:1<2:4<1:3<0:-1
u0,3 u3,8 t u2,9 u3,4 u2,9 | !0:3 !3:8 3:7 =3:7<2:9 =3:7<2:9 =3:7<2:9
u1,5 t t u1,7 u3,1 u3,8 u1,6 u3,5 u0,7 u0,6 t | !1:5 1:4 1:3 !1:7 !3:1<1:7 !3:8<1:7 =3:8<1:7 =3:8<1:7 =3:8<1:7 =3:8<1:7 3:7<1:6
u1,6 u3,6 u0,8 u3,9 u0,3 u0,-1 u0,6 u2,7 t t t u2,9 | !1:6 !3:6 =3:6<0:8 !3:9<0:8 =3:9<0:8 =3:9<0:-1 =3:9<0:-1 =3:9<0:-1 3:8<0:-1 3:7<0:-1 3:6<0:-1 =3:6<2:9<0:-1
u0,4 t u0,7 u2,6 u3,8 t u2,9 t u2,4 t u0,5 u2,1 u1,1 | !0:4 0:3 !0:7 !2:6<0:7 !3:8<0:7 3:7<0:6 =3:7<2:9 3:6<2:8 =3:6<2:8 3:5<2:7 =3:5<2:7 =3:5<2:7 =3:5<2:7
u0,5 u3,8 t | !0:5 !3:8 3:7
u1,7 u1,9 t | !1:7 !1:9 1:8
u3,9 u2,6 u1,-1 u1,7 u1,3 t | !3:9 =3:9 =3:9<1:-1 =3:9<1:-1 =3:9<1:-1 3:8<1:-1
u1,-1 t u2,1 u1,5 u1,7 t u1,1 t u1,6 u2,6 | !1:-1 1:-1 !2:1<1:-1 =2:1<1:-1 =2:1<1:-1 1:-1 =1:-1 1:-1 =1:-1 !2:6<1:-1
u2,1 u2,6 u2,2 t u1,3 t u1,4 t t t | !2:1 !2:6 =2:6 2:5 =2:5 2:4 =2:4 2:3 2:2 2:1
u2,7 t u0,9 u0,-1 u3,5 u2,-1 t u3,8 u0,9 t u3,7 u2,-1 u0,8 u2,9 u3,4 | !2:7 2:6 =2:6<0:9 =2:6<0:-1 !3:5<2:6<0:-1 =3:5<2:-1<0:-1 3:4<2:-1<0:-1 !3:8<2:-1<0:-1 =3:8<2:-1<0:-1 3:7<2:-1<0:-1 =3:7<2:-1<0:-1 =3:7<2:-1<0:-1 =3:7<2:-1<0:-1 =3:7<2:-1<0:-1 =3:7<2:-1<0:-1
u2,5 u0,4 u3,8 t t u3,3 u1,3 u0,2 u2,8 | !2:5 =2:5 !3:8 3:7 3:6 =3:6 =3:6 =3:6 =3:6<2:8
u1,4 u1,8 u0,1 t u2,7 t u3,8 t u0,1 u3,8 u0,6 u2,9 | !1:4 !1:8 =1:8 1:7 !2:7 2:6 !3:8 3:7 =3:7 !3:8 =3:8 =3:8<2:9
u0,1 u0,1 u0,8 t u0,4 u3,4 | !0:1 =0:1 !0:8 0:7 =0:7 !3:4<0:7
u0,-1 u1,5 t u2,1 u0,1 t t t t | !0:-1 !1:5<0:-1 1:4<0:-1 !2:1<1:4<0:-1 =2:1<1:4<0:-1 1:3<0:-1 1:2<0:-1 1:1<0:-1 0:-1
u2,4 u1,-1 t t u3,7 u0,2 u1,9 t | !2:4 =2:4<1:-1 2:3<1:-1 2:2<1:-1 !3:7<1:-1 =3:7<1:-1 =3:7<1:-1 3:6<1:-1
u1,5 u3,9 u3,7 u1,2 u0,1 u0,3 t u0,7 u2,-1 u0,3 t | !1:5 !3:9 =3:9 =3:9 =3:9 =3:9 3:8 =3:8 =3:8<2:-1 =3:8<2:-1 3:7<2:-1
u3,8 u2,1 u1,6 u0,3 u0,8 t u0,6 t u3,9 t u2,2 u2,3 | !3:8 =3:8 =3:8 =3:8 =3:8 3:7 =3:7 3:6 !3:9 3:8 =3:8 =3:8
u0,2 t u0,7 u0,5 | !0:2 0:1 !0:7 =0:7
u0,5 u1,-1 u3,1 u0,3 | !0:5 !1:-1 !3:1<1:-1 =3:1<1:-1
u2,4 u3,5 t u2,9 u1,4 u3,2 | !2:4 !3:5 3:4 =3:4<2:9 =3:4<2:9 =3:4<2:9
u3,1 u3,4 t u2,6 t u3,6 u0,4 t t u0,8 t u2,3 u2,9 t u0,1 u1,2 | !3:1 !3:4 3:3 =3:3<2:6 3:2<2:5 !3:6<2:5 =3:6<2:5 3:5<2:4 3:4<2:3 =3:4<2:3<0:8 3:3<2:2<0:7 =3:3<2:2<0:7 =3:3<2:9<0:7 3:2<2:8<0:6 =3:2<2:8<0:6 =3:2<2:8<0:6
u2,8 u2,9 u3,-1 u2,5 u1,2 u1,9 u0,8 t u1,9 u1,7 u0,5 t t u3,-1 u1,9 | !2:8 !2:9 !3:-1 =3:-1 =3:-1 =3:-1 =3:-1 3:-1 =3:-1 =3:-1 =3:-1 3:-1 3:-1 =3:-1 =3:-1
u2,3 t u1,7 u3,2 u0,3 t u2,7 t u3,6 t u1,9 | !2:3 2:2 =2:2<1:7 !3:2<1:7 =3:2<1:7 3:1<1:6 =3:1<2:7 2:6 !3:6 3:5 =3:5<1:9
u1,3 u1,5 u3,1 | !1:3 !1:5 !3:1<1:5
u1,4 t t u1,7 u3,3 u1,3 u1,7 u0,3 u3,7 t u0,9 u1,5 t u0,4 u2,6 u2,5 | !1:4 1:3 1:2 !1:7 !3:3<1:7 =3:3<1:7 =3:3<1:7 =3:3<1:7 !3:7<1:7 3:6<1:6 =3:6<1:6<0:9 =3:6<1:6<0:9 3:5<1:5<0:8 =3:5<1:5<0:8 =3:5<2:6<0:8 =3:5<2:6<0:8
u3,4 u0,4 u2,8 u2,7 u1,-1 u2,5 t t t t u0,4 t t u2,4 u0,8 | !3:4 =3:4 =3:4<2:8 =3:4<2:8 =3:4<2:8<1:-1 =3:4<2:8<1:-1 3:3<2:7<1:-1 3:2<2:6<1:-1 3:1<2:5<1:-1 2:4<1:-1 =2:4<1:-1 2:3<1:-1 2:2<1:-1 !2:4<1:-1 =2:4<1:-1
u2,2 u1,1 u3,8 u2,3 u3,4 t | !2:2 =2:2 !3:8 =3:8 =3:8 3:7
u1,8 t u0,-1 t t u2,9 u3,6 | !1:8 1:7 =1:7<0:-1 1:6<0:-1 1:5<0:-1 !2:9<0:-1 !3:6<2:9<0:-1
u1,5 u1,3 t t u2,3 u3,4 u0,1 u2,4 u3,2 t u0,2 u1,4 | !1:5 =1:5 1:4 1:3 !2:3 !3:4 =3:4 =3:4 =3:4 3:3 =3:3 =3:3<1:4
u3,5 u0,8 u2,9 u3,9 | !3:5 =3:5<0:8 =3:5<2:9 !3:9<2:9
u1,4 t t u3,5 u0,9 u0,2 u0,4 u2,7 u2,9 u1,3 u3,7 u1,-1 u2,8 u0,1 u3,7 u1,6 | !1:4 1:3 1:2 !3:5 =3:5<0:9 =3:5<0:9 =3:5<0:9 =3:5<2:7<0:9 =3:5<2:9<0:9 =3:5<2:9<0:9 !3:7<2:9<0:9 =3:7<2:9<1:-1 =3:7<2:9<1:-1 =3:7<2:9<1:-1 =3:7<2:9<1:-1 =3:7<2:9<1:-1
u1,3 u3,9 t u3,1 u1,6 u3,1 t u2,1 t u0,2 u3,7 u1,5 u3,1 u3,5 u2,9 u1,1 | !1:3 !3:9 3:8 =3:8 =3:8 =3:8 3:7 =3:7 3:6 =3:6 !3:7 =3:7 =3:7 =3:7 =3:7<2:9 =3:7<2:9
u0,7 u0,4 t | !0:7 =0:7 0:6
u0,1 u0,7 t u1,2 u3,5 | !0:1 !0:7 0:6 !1:2<0:6 !3:5<0:6
u0,-1 u2,7 t | !0:-1 !2:7<0:-1 2:6<0:-1
u1,1 u2,7 t t u0,7 t t t u2,3 u1,6 u1,6 u3,6 t t u0,-1 | !1:1 !2:7 2:6 2:5 =2:5<0:7 2:4<0:6 2:3<0:5 2:2<0:4 !2:3<0:4 =2:3<1:6 =2:3<1:6 !3:6<1:6 3:5<1:5 3:4<1:4 =3:4<1:4<0:-1
u1,1 u3,-1 u1,8 u2,8 t u3,5 u0,3 t u3,8 u3,7 | !1:1 !3:-1 =3:-1 =3:-1 3:-1 =3:-1 =3:-1 3:-1 =3:-1 =3:-1
u1,9 u1,-1 u0,8 u1,6 u1,6 | !1:9 !1:-1 =1:-1 =1:-1 =1:-1
u0,6 t u1,1 t u0,4 u0,-1 t | !0:6 0:5 !1:1<0:5 0:4 =0:4 !0:-1 0:-1
u0,4 u1,7 u1,1 t u1,3 t t u0,7 u1,7 u0,5 u1,6 u1,2 | !0:4 !1:7 =1:7 1:6 =1:6 1:5 1:4 =1:4<0:7 !1:7<0:7 =1:7<0:7 =1:7<0:7 =1:7<0:7
u2,9 u2,7 u2,6 u0,8 u0,7 | !2:9 =2:9 =2:9 =2:9 =2:9
u2,1 t u3,9 t u0,8 u0,2 | !2:1 X !3:9 3:8 =3:8 =3:8
u0,6 t t u1,6 t t | !0:6 0:5 0:4 !1:6 1:5 1:4
u0,-1 u2,9 t u3,7 u1,4 | !0:-1 !2:9<0:-1 2:8<0:-1 !3:7<2:8<0:-1 =3:7<2:8<0:-1
u0,9 u3,7 t t u1,8 u3,3 u0,9 t u1,9 u0,8 u1,5 t | !0:9 !3:7<0:9 3:6<0:8 3:5<0:7 =3:5<1:8 =3:5<1:8 =3:5<1:8<0:9 3:4<1:7<0:8 =3:4<1:9<0:8 =3:4<1:9<0:8 =3:4<1:9<0:8 3:3<1:8<0:7
u3,6 u1,9 u0,4 u1,7 u2,2 u1,2 u3,-1 u3,6 t u2,6 | !3:6 =3:6<1:9 =3:6<1:9 =3:6<1:9 =3:6<1:9 =3:6<1:9 !3:-1<1:9 =3:-1<1:9 3:-1<1:8 =3:-1<1:8
u1,9 t t t u0,4 t u3,7 u1,9 t t u1,2 u2,6 | !1:9 1:8 1:7 1:6 =1:6 1:5 !3:7 =3:7<1:9 3:6<1:8 3:5<1:7 =3:5<1:7 =3:5<2:6<1:7
u2,-1 t u0,6 t u2,-1 u1,5 u1,7 u3,2 t t u1,7 | !2:-1 2:-1 =2:-1 2:-1 =2:-1 =2:-1 =2:-1 !3:2<2:-1 3:1<2:-1 2:-1 =2:-1
u2,5 u2,2 t u2,7 u0,7 u0,-1 u0,7 t u3,-1 u0,1 | !2:5 =2:5 2:4 !2:7 =2:7 =2:7<0:-1 =2:7<0:-1 2:6<0:-1 !3:-1<0:-1 =3:-1<0:-1
u0,-1 u2,8 t t u3,6 u3,3 u2,7 u1,5 u3,5 | !0:-1 !2:8<0:-1 2:7<0:-1 2:6<0:-1 !3:6<0:-1 =3:6<0:-1 =3:6<2:7<0:-1 =3:6<2:7<0:-1 =3:6<2:7<0:-1
u3,4 u2,1 t t u0,-1 t u3,1 u3,7 u0,4 u3,9 u2,4 u0,4 u3,6 u3,5 u2,8 | !3:4 =3:4 3:3 3:2 =3:2<0:-1 3:1<0:-1 =3:1<0:-1 !3:7<0:-1 =3:7<0:-1 !3:9<0:-1 =3:9<0:-1 =3:9<0:-1 =3:9<0:-1 =3:9<0:-1 =3:9<0:-1
u0,-1 u1,4 t u2,2 t | !0:-1 !1:4<0:-1 1:3<0:-1 !2:2<1:3<0:-1 2:1<1:2<0:-1
u3,9 u1,6 t u2,7 t t t u0,3 | !3:9 =3:9 3:8 =3:8 3:7 3:6 3:5 =3:5
u0,7 u1,7 u3,-1 u2,3 u1,-1 t t u3,2 u1,3 t u3,1 u3,3 u3,4 u3,8 u3,1 u1,-1 | !0:7 !1:7 !3:-1 =3:-1 =3:-1 3:-1 3:-1 =3:-1 =3:-1 3:-1 =3:-1 =3:-1 =3:-1 =3:-1 =3:-1 =3:-1
u0,2 t t u1,7 u2,1 | !0:2 0:1 X !1:7 !2:1<1:7
u2,-1 u0,3 u2,1 t t t u0,4 u2,6 t t t u2,8 t u0,6 t | !2:-1 =2:-1 =2:-1 2:-1 2:-1 2:-1 =2:-1 =2:-1 2:-1 2:-1 2:-1 =2:-1 2:-1 =2:-1 2:-1
u2,5 u3,-1 t u3,7 t u3,9 u1,1 u3,-1 u2,4 u3,-1 t u3,2 u3,5 u2,2 t | !2:5 !3:-1 3:-1 =3:-1 3:-1 =3:-1 =3:-1 =3:-1 =3:-1 =3:-1 3:-1 =3:-1 =3:-1 =3:-1 3:-1
u2,9 u0,9 u3,9 u3,-1 u0,6 u0,6 u3,3 u2,1 u0,1 | !2:9 =2:9 !3:9 !3:-1 =3:-1 =3:-1 =3:-1 =3:-1 =3:-1
u2,8 u0,6 t t u2,9 u2,5 u0,8 u3,6 u3,1 t | !2:8 =2:8 2:7 2:6 !2:9 =2:9 =2:9 !3:6<2:9 =3:6<2:9 3:5<2:8
u3,4 u2,4 u3,9 u1,5 u3,5 t u2,2 u1,5 t t u2,1 t u2,6 u3,1 | !3:4 =3:4 !3:9 =3:9 =3:9 3:8 =3:8 =3:8 3:7 3:6 =3:6 3:5 =3:5<2:6 =3:5<2:6
u2,2 u0,8 t u0,8 u0,6 u0,8 | !2:2 =2:2<0:8 2:1<0:7 =2:1<0:8 =2:1<0:8 =2:1<0:8
u0,4 t u0,5 u2,7 | !0:4 0:3 !0:5 !2:7
u0,6 t u1,4 u1,-1 t t u2,8 | !0:6 0:5 !1:4<0:5 !1:-1<0:5 1:-1<0:4 1:-1<0:3 !2:8<1:-1<0:3
u3,6 u0,6 u3,6 u3,5 u0,5 u1,-1 | !3:6 =3:6 =3:6 =3:6 =3:6 =3:6<1:-1
u0,8 t u2,5 u3,8 u1,3 t u0,6 t u0,9 t t t | !0:8 0:7 !2:5<0:7 !3:8<0:7 =3:8<0:7 3:7<0:6 =3:7<0:6 3:6<0:5 =3:6<0:9 3:5<0:8 3:4<0:7 3:3<0:6
u0,-1 u3,5 t u3,7 | !0:-1 !3:5<0:-1 3:4<0:-1 !3:7<0:-1
u2,-1 t u1,3 u3,7 | !2:-1 2:-1 =2:-1 !3:7<2:-1
u1,5 u3,7 u3,3 u0,2 t t t u2,9 u0,5 u0,6 u3,4 t | !1:5 !3:7 =3:7 =3:7 3:6 3:5 3:4 =3:4<2:9 =3:4<2:9 =3:4<2:9 =3:4<2:9 3:3<2:8
u3,5 t u3,6 u1,6 u0,9 t u3,1 u2,6 | !3:5 3:4 !3:6 =3:6 =3:6<0:9 3:5<0:8 =3:5<0:8 =3:5<2:6<0:8
u3,2 u0,8 u2,2 t u0,7 u3,8 t u0,-1 t u1,7 t u2,2 t | !3:2 =3:2<0:8 =3:2<0:8 3:1<0:7 =3:1<0:7 !3:8<0:7 3:7<0:6 =3:7<0:-1 3:6<0:-1 =3:6<1:7<0:-1 3:5<1:6<0:-1 =3:5<1:6<0:-1 3:4<1:5<0:-1
u2,-1 u0,2 u1,3 u3,5 u2,9 u1,1 u2,7 u3,6 | !2:-1 =2:-1 =2:-1 !3:5<2:-1 =3:5<2:-1 =3:5<2:-1 =3:5<2:-1 !3:6<2:-1
u3,3 u3,5 u3,6 u2,2 u3,5 t u0,5 t t t | !3:3 !3:5 !3:6 =3:6 =3:6 3:5 =3:5 3:4 3:3 3:2
u3,4 u0,4 t u3,6 t u0,3 u2,6 u1,5 u3,4 u0,-1 u2,8 u1,5 u1,8 t u0,2 u2,9 | !3:4 =3:4 3:3 !3:6 3:5 =3:5 =3:5<2:6 =3:5<2:6 =3:5<2:6 =3:5<2:6<0:-1 =3:5<2:8<0:-1 =3:5<2:8<0:-1 =3:5<2:8<0:-1 3:4<2:7<0:-1 =3:4<2:7<0:-1 =3:4<2:9<0:-1
u2,-1 u0,9 u1,-1 u1,4 u1,4 t u0,9 t u2,9 u3,8 t u1,1 u0,7 | !2:-1 =2:-1 =2:-1 =2:-1 =2:-1 2:-1 =2:-1 2:-1 =2:-1 !3:8<2:-1 3:7<2:-1 =3:7<2:-1 =3:7<2:-1
u1,3 u2,9 u1,1 t u2,8 t u0,3 u3,3 u2,-1 u0,9 | !1:3 !2:9 =2:9 2:8 =2:8 2:7 =2:7 !3:3<2:7 =3:3<2:-1 =3:3<2:-1
u0,7 u0,-1 u1,9 u0,3 t u0,6 t t u2,4 t u1,5 t | !0:7 !0:-1 !1:9<0:-1 =1:9<0:-1 1:8<0:-1 =1:8<0:-1 1:7<0:-1 1:6<0:-1 !2:4<1:6<0:-1 2:3<1:5<0:-1 =2:3<1:5<0:-1 2:2<1:4<0:-1
u2,8 t u0,-1 | !2:8 2:7 =2:7<0:-1
u3,7 u0,2 u3,4 t u0,2 u1,4 u3,5 u1,6 t u1,2 u0,4 u0,3 | !3:7 =3:7 =3:7 3:6 =3:6 =3:6 =3:6 =3:6 3:5 =3:5 =3:5 =3:5
u3,8 u1,2 t t t t t u0,9 t u3,5 | !3:8 =3:8 3:7 3:6 3:5 3:4 3:3 =3:3<0:9 3:2<0:8 !3:5<0:8
u3,8 u1,7 t u1,5 u0,-1 u1,4 t u1,8 u0,-1 u1,-1 t u0,1 u2,2 u3,1 u1,5 | !3:8 =3:8 3:7 =3:7 =3:7<0:-1 =3:7<0:-1 3:6<0:-1 =3:6<1:8<0:-1 =3:6<1:8<0:-1 =3:6<1:-1<0:-1 3:5<1:-1<0:-1 =3:5<1:-1<0:-1 =3:5<1:-1<0:-1 =3:5<1:-1<0:-1 =3:5<1:-1<0:-1
u0,5 t u3,3 u1,9 | !0:5 0:4 !3:3<0:4 =3:3<1:9
u0,1 t u1,-1 u0,7 u2,7 u1,4 u0,4 t | !0:1 X !1:-1 =1:-1 !2:7<1:-1 =2:7<1:-1 =2:7<1:-1 2:6<1:-1
u2,7 t t u1,5 | !2:7 2:6 2:5 =2:5
u0,2 u0,8 u1,7 t u1,1 t u3,5 u2,4 | !0:2 !0:8 !1:7<0:8 1:6<0:7 =1:6<0:7 1:5<0:6 !3:5<0:6 =3:5<0:6
u0,8 u3,4 u0,-1 u2,2 t t u0,3 u1,4 u1,-1 u2,6 u1,5 u1,-1 u2,9 u2,8 | !0:8 !3:4<0:8 =3:4<0:-1 =3:4<0:-1 3:3<0:-1 3:2<0:-1 =3:2<0:-1 =3:2<1:4<0:-1 =3:2<1:-1<0:-1 =3:2<2:6<1:-1<0:-1 =3:2<2:6<1:-1<0:-1 =3:2<2:6<1:-1<0:-1 =3:2<2:9<1:-1<0:-1 =3:2<2:9<1:-1<0:-1
u3,9 t u0,2 u3,7 u2,6 u2,5 t t u1,4 u3,3 u3,4 t u2,1 u3,4 t u0,6 | !3:9 3:8 =3:8 =3:8 =3:8 =3:8 3:7 3:6 =3:6 =3:6 =3:6 3:5 =3:5 =3:5 3:4 =3:4<0:6
u2,8 t t u2,7 t t u0,1 u0,4 u3,1 t u1,2 u0,-1 | !2:8 2:7 2:6 !2:7 2:6 2:5 =2:5 =2:5 !3:1<2:5 2:4 =2:4 =2:4<0:-1
u1,3 t u1,8 u0,1 | !1:3 1:2 !1:8 =1:8
u1,8 u1,9 u0,2 | !1:8 !1:9 =1:9
u3,3 t t t u0,2 t u3,1 u2,9 u0,-1 t | !3:3 3:2 3:1 X !0:2 0:1 !3:1 =3:1<2:9 =3:1<2:9<0:-1 2:8<0:-1
u0,1 u0,6 u3,7 u3,7 t u3,5 u0,2 u0,9 u2,2 t t t t u3,7 u0,3 | !0:1 !0:6 !3:7 =3:7 3:6 =3:6 =3:6 =3:6<0:9 =3:6<0:9 3:5<0:8 3:4<0:7 3:3<0:6 3:2<0:5 !3:7<0:5 =3:7<0:5
u0,2 t u2,5 t t u2,6 u2,3 u1,7 u0,7 t u3,9 t u2,2 | !0:2 0:1 !2:5 2:4 2:3 !2:6 =2:6 =2:6<1:7 =2:6<1:7 2:5<1:6 !3:9<1:6 3:8<1:5 =3:8<1:5
u1,6 t u1,8 u0,4 t u1,8 u3,8 t | !1:6 1:5 !1:8 =1:8 1:7 !1:8 !3:8 3:7
u2,7 u1,8 u0,1 t t u0,1 u2,8 | !2:7 =2:7<1:8 =2:7<1:8 2:6<1:7 2:5<1:6 =2:5<1:6 !2:8<1:6
u3,9 u1,6 u3,2 u2,8 u3,3 u1,5 t t u0,-1 t u0,7 u2,6 u2,2 | !3:9 =3:9 =3:9 =3:9 =3:9 =3:9 3:8 3:7 =3:7<0:-1 3:6<0:-1 =3:6<0:-1 =3:6<0:-1 =3:6<0:-1
u1,2 t u3,3 u2,9 t t u2,9 t u1,8 t u0,5 u1,8 t u3,9 | !1:2 1:1 !3:3 =3:3<2:9 3:2<2:8 3:1<2:7 =3:1<2:9 2:8 =2:8 2:7 =2:7 =2:7<1:8 2:6<1:7 !3:9<1:7
u1,5 t u1,1 | !1:5 1:4 =1:4
u2,1 u1,4 u3,1 t t t u2,8 u0,6 t | !2:1 =2:1<1:4 !3:1<1:4 1:3 1:2 1:1 !2:8 =2:8 2:7
u3,-1 u0,3 u3,9 t t u3,9 | !3:-1 =3:-1 =3:-1 3:-1 3:-1 =3:-1
u3,-1 u2,5 u0,5 | !3:-1 =3:-1 =3:-1
u1,9 u1,2 u3,5 u0,9 t u2,4 u0,4 t u0,3 t u2,7 t | !1:9 =1:9 !3:5<1:9 =3:5<1:9 3:4<1:8 =3:4<1:8 =3:4<1:8 3:3<1:7 =3:3<1:7 3:2<1:6 =3:2<2:7 3:1<2:6
u0,4 t u0,8 t t | !0:4 0:3 !0:8 0:7 0:6
u2,4 u2,6 u2,2 u0,-1 t u2,3 u1,1 t t | !2:4 !2:6 =2:6 =2:6<0:-1 2:5<0:-1 =2:5<0:-1 =2:5<0:-1 2:4<0:-1 2:3<0:-1
u0,6 u3,4 u1,8 t u2,6 | !0:6 !3:4<0:6 =3:4<1:8 3:3<1:7 =3:3<2:6<1:7
u2,8 u1,1 t u1,9 u2,3 t t u0,2 u0,4 u2,-1 t | !2:8 =2:8 2:7 =2:7<1:9 =2:7<1:9 2:6<1:8 2:5<1:7 =2:5<1:7 =2:5<1:7 !2:-1<1:7 2:-1<1:6
u0,2 u1,4 t t t u2,5 u1,5 u0,1 u1,8 u3,8 | !0:2 !1:4 1:3 1:2 1:1 !2:5 =2:5 =2:5 =2:5<1:8 !3:8<1:8
u0,4 u3,6 u2,4 u3,4 t u3,5 t u3,6 u3,8 | !0:4 !3:6 =3:6 =3:6 3:5 =3:5 3:4 !3:6 !3:8
u0,3 u2,9 u0,2 t u0,8 u2,4 u0,4 u0,9 | !0:3 !2:9 =2:9 2:8 =2:8 =2:8 =2:8 =2:8<0:9
u0,9 t u0,2 u0,5 | !0:9 0:8 =0:8 =0:8
u2,2 u3,4 t t u3,3 u0,7 u1,9 | !2:2 !3:4 3:3 3:2 !3:3 =3:3<0:7 =3:3<1:9
u0,8 t t u1,9 u1,5 | !0:8 0:7 0:6 !1:9 =1:9
u0,6 u1,7 u3,8 u0,-1 u3,1 u1,6 u1,4 t u1,4 u0,-1 u0,4 t | !0:6 !1:7 !3:8 =3:8<0:-1 =3:8<0:-1 =3:8<0:-1 =3:8<0:-1 3:7<0:-1 =3:7<0:-1 =3:7<0:-1 =3:7<0:-1 3:6<0:-1
u1,1 u3,7 u3,9 | !1:1 !3:7 !3:9
u0,8 u0,6 t t u0,5 t t u0,6 u1,-1 u1,7 u1,1 u3,6 u3,8 u3,6 u0,6 | !0:8 =0:8 0:7 0:6 =0:6 0:5 0:4 !0:6 !1:-1 =1:-1 =1:-1 !3:6<1:-1 !3:8<1:-1 =3:8<1:-1 =3:8<1:-1
u0,3 t u0,8 u3,9 u0,8 u0,3 t u0,6 u0,1 u1,7 u1,9 t u2,7 u2,8 u1,8 | !0:3 0:2 !0:8 !3:9 =3:9 =3:9 3:8 =3:8 =3:8 =3:8 =3:8<1:9 3:7<1:8 =3:7<1:8 =3:7<2:8 =3:7<2:8
u0,7 u1,2 u2,1 t u1,5 t u0,8 u3,6 t t t u3,7 u3,7 t t t | !0:7 !1:2<0:7 !2:1<1:2<0:7 1:1<0:6 !1:5<0:6 1:4<0:5 =1:4<0:8 !3:6<0:8 3:5<0:7 3:4<0:6 3:3<0:5 !3:7<0:5 =3:7<0:5 3:6<0:4 3:5<0:3 3:4<0:2
u0,6 u3,7 u0,6 t u2,-1 u0,2 | !0:6 !3:7 =3:7 3:6 =3:6<2:-1 =3:6<2:-1
u1,4 u2,2 t t u2,4 t u1,9 t u0,1 u2,6 u2,5 | !1:4 !2:2<1:4 2:1<1:3 1:2 !2:4 2:3 =2:3<1:9 2:2<1:8 =2:2<1:8 !2:6<1:8 =2:6<1:8
u3,-1 u3,1 u3,4 u0,8 u0,7 | !3:-1 =3:-1 =3:-1 =3:-1 =3:-1
u1,4 u0,6 u0,9 u0,7 | !1:4 =1:4<0:6 =1:4<0:9 =1:4<0:9
u2,7 u2,9 u1,-1 t | !2:7 !2:9 =2:9<1:-1 2:8<1:-1
u0,6 u0,1 u3,9 | !0:6 =0:6 !3:9
u1,7 u0,6 t u3,3 t t t u3,6 | !1:7 =1:7 1:6 !3:3<1:6 3:2<1:5 3:1<1:4 1:3 !3:6
u3,-1 u0,1 t u2,9 t u2,9 u0,2 t t u2,7 t u0,4 u0,7 u2,9 u1,3 u1,3 | !3:-1 =3:-1 3:-1 =3:-1 3:-1 =3:-1 =3:-1 3:-1 3:-1 =3:-1 3:-1 =3:-1 =3:-1 =3:-1 =3:-1 =3:-1
u2,9 u1,7 t u3,5 t u1,8 u2,5 u0,3 u1,1 u1,5 | !2:9 =2:9 2:8 !3:5<2:8 3:4<2:7 =3:4<2:7<1:8 =3:4<2:7<1:8 =3:4<2:7<1:8 =3:4<2:7<1:8 =3:4<2:7<1:8
u3,8 u2,6 t t u2,9 t u1,2 t u2,1 u3,-1 u0,3 u3,9 | !3:8 =3:8 3:7 3:6 =3:6<2:9 3:5<2:8 =3:5<2:8 3:4<2:7 =3:4<2:7 !3:-1<2:7 =3:-1<2:7 =3:-1<2:7
u2,9 u3,-1 u1,1 t t t u0,2 u3,5 t | !2:9 !3:-1 =3:-1 3:-1 3:-1 3:-1 =3:-1 =3:-1 3:-1
u2,7 u1,2 u2,8 u0,7 u3,2 t u3,2 t u1,3 t | !2:7 =2:7 !2:8 =2:8 !3:2<2:8 3:1<2:7 !3:2<2:7 3:1<2:6 =3:1<2:6 2:5
u0,1 u3,6 t u1,-1 u1,4 u0,2 u3,2 u2,9 u0,7 u1,2 u3,7 t u3,9 t t u1,4 | !0:1 !3:6 3:5 =3:5<1:-1 =3:5<1:-1 =3:5<1:-1 =3:5<1:-1 =3:5<2:9<1:-1 =3:5<2:9<1:-1 =3:5<2:9<1:-1 !3:7<2:9<1:-1 3:6<2:8<1:-1 !3:9<2:8<1:-1 3:8<2:7<1:-1 3:7<2:6<1:-1 =3:7<2:6<1:-1
u1,7 t u1,6 t u0,3 t t u1,7 t u2,3 | !1:7 1:6 =1:6 1:5 =1:5 1:4 1:3 !1:7 1:6 !2:3<1:6
";

    /// `amplifier:duration<...` for the speed effect and whatever it covers.
    fn chain_string(p: &PlayerState) -> String {
        let i = p.effects.list.iter().position(|e| e.id == SPEED).unwrap();
        let e = &p.effects.list[i];
        let mut s = format!("{}:{}", e.amplifier, e.duration);
        let mut cur = p.effects.hidden[i].as_deref();
        while let Some(h) = cur {
            s += &format!("<{}:{}", h.amplifier, h.duration);
            cur = h.below.as_deref();
        }
        s
    }

    #[test]
    fn effect_list_matches_mob_effect_instance() {
        let mut steps = 0;
        let mut covered = 0;
        for line in EFFECT_CASES.lines() {
            let (ops, states) = line.split_once('|').expect("case format");
            let mut p = player();
            for (op, want) in ops.split_whitespace().zip(states.split_whitespace()) {
                let got = if op == "t" {
                    tick_effects(&mut p);
                    if p.effects.has(SPEED) {
                        chain_string(&p)
                    } else {
                        "X".to_string()
                    }
                } else {
                    let (a, d) = op[1..].split_once(',').expect("op format");
                    let changed = add_effect(&mut p, SPEED, a.parse().unwrap(), d.parse().unwrap());
                    format!("{}{}", if changed { '!' } else { '=' }, chain_string(&p))
                };
                assert_eq!(got, want, "{line}");
                if got.matches('<').count() > 0 {
                    covered += 1;
                }
                // The attribute modifier always follows the effect actually in force.
                let want_speed = match p.effects.amplifier(SPEED) {
                    Some(amp) => {
                        f64::from(0.1_f32) * (1.0 + f64::from(0.2_f32) * f64::from(amp + 1))
                    }
                    None => f64::from(0.1_f32),
                };
                assert_eq!(ms(&p).to_bits(), want_speed.to_bits(), "{line}");
                steps += 1;
            }
        }
        assert!(steps > 1000, "{steps} steps");
        assert!(covered > 200, "{covered} steps with a covered effect");
    }
}
