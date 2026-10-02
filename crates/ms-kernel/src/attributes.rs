//! Entity attributes (`AttributeInstance`): a base value plus modifiers grouped by operation,
//! combined exactly as the game does — add values, then base-multipliers, then each
//! total-multiplier in turn. Status effects and sprinting act on movement through modifiers here.

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

    /// The player's base value for this attribute.
    pub fn player_base(self) -> f64 {
        match self {
            Attribute::MovementSpeed => f64::from(0.1_f32),
            Attribute::JumpStrength => 0.42,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    /// The modifier's identifier, e.g. `"minecraft:sprinting"`.
    pub id: String,
    pub amount: f64,
    pub operation: Operation,
}

#[derive(Clone, Debug, PartialEq)]
struct Instance {
    base: f64,
    modifiers: Vec<Modifier>,
}

/// All of an entity's attribute instances.
#[derive(Clone, Debug, PartialEq)]
pub struct Attributes {
    instances: Vec<(Attribute, Instance)>,
}

impl Attributes {
    /// The player's attributes at their base values, without modifiers.
    pub fn player() -> Self {
        Self {
            instances: Attribute::ALL
                .iter()
                .map(|&a| {
                    (
                        a,
                        Instance {
                            base: a.player_base(),
                            modifiers: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn instance(&self, a: Attribute) -> &Instance {
        &self
            .instances
            .iter()
            .find(|(k, _)| *k == a)
            .expect("all attributes present")
            .1
    }

    fn instance_mut(&mut self, a: Attribute) -> &mut Instance {
        &mut self
            .instances
            .iter_mut()
            .find(|(k, _)| *k == a)
            .expect("all attributes present")
            .1
    }

    pub fn base(&self, a: Attribute) -> f64 {
        self.instance(a).base
    }

    pub fn set_base(&mut self, a: Attribute, value: f64) {
        self.instance_mut(a).base = value;
    }

    pub fn modifiers(&self, a: Attribute) -> &[Modifier] {
        &self.instance(a).modifiers
    }

    /// Add (or replace) a modifier.
    pub fn add_modifier(&mut self, a: Attribute, modifier: Modifier) {
        let inst = self.instance_mut(a);
        inst.modifiers.retain(|m| m.id != modifier.id);
        inst.modifiers.push(modifier);
    }

    pub fn remove_modifier(&mut self, a: Attribute, id: &str) {
        self.instance_mut(a).modifiers.retain(|m| m.id != id);
    }

    pub fn has_modifier(&self, a: Attribute, id: &str) -> bool {
        self.instance(a).modifiers.iter().any(|m| m.id == id)
    }

    /// `AttributeInstance.getValue`.
    pub fn value(&self, a: Attribute) -> f64 {
        let inst = self.instance(a);
        let of = |op: Operation| inst.modifiers.iter().filter(move |m| m.operation == op);
        let mut d = inst.base;
        for m in of(Operation::AddValue) {
            d += m.amount;
        }
        let mut e = d;
        for m in of(Operation::AddMultipliedBase) {
            e += d * m.amount;
        }
        for m in of(Operation::AddMultipliedTotal) {
            e *= 1.0 + m.amount;
        }
        sanitize(a, e)
    }
}

/// `RangedAttribute.sanitizeValue`: clamp into the attribute's range (NaN → min).
fn sanitize(a: Attribute, v: f64) -> f64 {
    let (min, max) = match a {
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
    };
    if v.is_nan() {
        min
    } else {
        v.clamp(min, max)
    }
}
