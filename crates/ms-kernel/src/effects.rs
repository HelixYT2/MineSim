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
