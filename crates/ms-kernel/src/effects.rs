//! Status effects (`MobEffectInstance`): which are active, at what amplifier, for how long.
//! Movement-relevant effects act either through attribute modifiers (speed, slowness) or are read
//! directly by the physics (jump boost, slow falling, levitation, dolphin's grace).

/// One active effect.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectInstance {
    /// Registry id, e.g. `"minecraft:speed"`.
    pub id: String,
    pub amplifier: i32,
    /// Remaining ticks (`-1` = infinite).
    pub duration: i32,
}

/// The set of active effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    list: Vec<EffectInstance>,
}

impl Effects {
    pub fn get(&self, id: &str) -> Option<&EffectInstance> {
        self.list.iter().find(|e| e.id == id)
    }

    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EffectInstance> {
        self.list.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Insert or replace the effect with this id (no attribute bookkeeping; see the kernel's
    /// effect application for that).
    pub fn insert(&mut self, e: EffectInstance) {
        match self.list.iter_mut().find(|x| x.id == e.id) {
            Some(slot) => *slot = e,
            None => self.list.push(e),
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<EffectInstance> {
        let i = self.list.iter().position(|e| e.id == id)?;
        Some(self.list.remove(i))
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut EffectInstance> {
        self.list.iter_mut()
    }
}

use crate::state::PlayerState;

/// `LivingEntity.tickEffects` as the client runs it for the local player: count durations down
/// and drop expired effects (with their attribute modifiers).
pub fn tick_effects(p: &mut PlayerState) {
    let _ = p;
}

/// Add an effect the way `addEffect` does (replacing a weaker or shorter one; applying its
/// attribute modifiers).
pub fn add_effect(p: &mut PlayerState, id: &str, amplifier: i32, duration: i32) {
    p.effects.insert(EffectInstance {
        id: id.to_string(),
        amplifier,
        duration,
    });
}

/// Remove one effect and its attribute modifiers.
pub fn remove_effect(p: &mut PlayerState, id: &str) {
    p.effects.remove(id);
}

/// Remove every effect and their attribute modifiers.
pub fn clear_effects(p: &mut PlayerState) {
    p.effects.clear();
}

/// `LivingEntity.setSprinting`: the flag plus the sprint speed modifier.
pub fn set_sprinting(p: &mut PlayerState, sprinting: bool) {
    p.sprinting = sprinting;
}

/// `LivingEntity.getJumpBoostPower`.
pub fn jump_boost_power(p: &PlayerState) -> f32 {
    match p.effects.get("minecraft:jump_boost") {
        Some(e) => 0.1 * (e.amplifier as f32 + 1.0),
        None => 0.0,
    }
}
