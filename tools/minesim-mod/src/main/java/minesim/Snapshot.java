package minesim;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.tags.FluidTags;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.projectile.Projectile;
import net.minecraft.world.entity.projectile.arrow.AbstractArrow;
import net.minecraft.world.food.FoodData;
import net.minecraft.world.phys.Vec3;

import java.util.LinkedHashMap;
import java.util.Map;

// The complete per-tick physics state of an entity, as the oracle corpus records it. Every float and
// double is its raw IEEE-754 bit pattern so it round-trips into the simulator exactly. Field names are
// short because the corpus is committed; docs/corpus.md is the key.
public final class Snapshot {
	// The attributes that feed movement, jumping, falling and damage, keyed by the short name the
	// corpus uses. Anything not listed here does not influence the simulated state.
	public static final Map<String, Holder<Attribute>> ATTRIBUTES = new LinkedHashMap<>();

	static {
		ATTRIBUTES.put("movement_speed", Attributes.MOVEMENT_SPEED);
		ATTRIBUTES.put("jump_strength", Attributes.JUMP_STRENGTH);
		ATTRIBUTES.put("gravity", Attributes.GRAVITY);
		ATTRIBUTES.put("step_height", Attributes.STEP_HEIGHT);
		ATTRIBUTES.put("safe_fall_distance", Attributes.SAFE_FALL_DISTANCE);
		ATTRIBUTES.put("fall_damage_multiplier", Attributes.FALL_DAMAGE_MULTIPLIER);
		ATTRIBUTES.put("knockback_resistance", Attributes.KNOCKBACK_RESISTANCE);
		ATTRIBUTES.put("water_movement_efficiency", Attributes.WATER_MOVEMENT_EFFICIENCY);
		ATTRIBUTES.put("movement_efficiency", Attributes.MOVEMENT_EFFICIENCY);
		ATTRIBUTES.put("sneaking_speed", Attributes.SNEAKING_SPEED);
		ATTRIBUTES.put("max_health", Attributes.MAX_HEALTH);
		ATTRIBUTES.put("armor", Attributes.ARMOR);
		ATTRIBUTES.put("scale", Attributes.SCALE);
		ATTRIBUTES.put("burning_time", Attributes.BURNING_TIME);
	}

	private Snapshot() {
	}

	public static long d(double v) {
		return Double.doubleToRawLongBits(v);
	}

	public static int f(float v) {
		return Float.floatToRawIntBits(v);
	}

	public static int b(boolean v) {
		return v ? 1 : 0;
	}

	// Position, velocity, rotation and the collision/ground flags every entity has.
	public static JsonObject entity(Entity e) {
		JsonObject o = new JsonObject();
		o.addProperty("x", d(e.getX()));
		o.addProperty("y", d(e.getY()));
		o.addProperty("z", d(e.getZ()));
		Vec3 v = e.getDeltaMovement();
		o.addProperty("dx", d(v.x));
		o.addProperty("dy", d(v.y));
		o.addProperty("dz", d(v.z));
		o.addProperty("yaw", f(e.getYRot()));
		o.addProperty("pitch", f(e.getXRot()));
		o.addProperty("ground", b(e.onGround()));
		o.addProperty("hc", b(e.horizontalCollision));
		o.addProperty("mhc", b(e.minorHorizontalCollision));
		o.addProperty("vc", b(e.verticalCollision));
		o.addProperty("vcb", b(e.verticalCollisionBelow));
		o.addProperty("fall", d(e.fallDistance));
		o.addProperty("water", b(e.isInWater()));
		o.addProperty("eyeWater", b(e.isUnderWater()));
		o.addProperty("lava", b(e.isInLava()));
		o.addProperty("waterH", d(e.getFluidHeight(FluidTags.WATER)));
		o.addProperty("lavaH", d(e.getFluidHeight(FluidTags.LAVA)));
		o.addProperty("powder", b(e.isInPowderSnow));
		o.addProperty("wasPowder", b(e.wasInPowderSnow));
		Vec3 stuck = e.stuckSpeedMultiplier;
		o.addProperty("stuckX", d(stuck.x));
		o.addProperty("stuckY", d(stuck.y));
		o.addProperty("stuckZ", d(stuck.z));
		if (e.mainSupportingBlockPos.isPresent()) {
			BlockPos p = e.mainSupportingBlockPos.get();
			o.addProperty("support", p.asLong());
		}
		o.addProperty("noBlocks", b(e.onGroundNoBlocks));
		o.addProperty("pose", e.getPose().name());
		o.addProperty("w", f(e.getBbWidth()));
		o.addProperty("h", f(e.getBbHeight()));
		o.addProperty("sprinting", b(e.isSprinting()));
		o.addProperty("shift", b(e.isShiftKeyDown()));
		o.addProperty("swimming", b(e.isSwimming()));
		o.addProperty("fire", e.getRemainingFireTicks());
		o.addProperty("frozen", e.getTicksFrozen());
		o.addProperty("age", e.tickCount);
		o.addProperty("invul", e.invulnerableTime);
		return o;
	}

	// Everything above plus the living-entity state: health and damage cooldowns, jump bookkeeping,
	// the movement input the entity is acting on, effects, and the movement attributes.
	public static JsonObject living(LivingEntity e) {
		JsonObject o = entity(e);
		o.addProperty("health", f(e.getHealth()));
		o.addProperty("absorption", f(e.getAbsorptionAmount()));
		o.addProperty("hurtTime", e.hurtTime);
		o.addProperty("lastHurt", f(e.lastHurt));
		o.addProperty("deathTime", e.deathTime);
		o.addProperty("njd", e.noJumpDelay);
		o.addProperty("jumping", b(e.jumping));
		o.addProperty("xxa", f(e.xxa));
		o.addProperty("zza", f(e.zza));
		o.addProperty("speed", f(e.getSpeed()));
		o.addProperty("climbable", b(e.onClimbable()));
		o.addProperty("fallFlying", b(e.isFallFlying()));
		o.add("effects", effects(e));
		JsonObject attrs = new JsonObject();
		for (Map.Entry<String, Holder<Attribute>> a : ATTRIBUTES.entrySet()) {
			if (e.getAttributes().hasAttribute(a.getValue())) {
				attrs.addProperty(a.getKey(), d(e.getAttributeValue(a.getValue())));
			}
		}
		o.add("attrs", attrs);
		if (e instanceof Player p) {
			FoodData food = p.getFoodData();
			o.addProperty("food", food.getFoodLevel());
			o.addProperty("saturation", f(food.getSaturationLevel()));
			o.addProperty("exhaustion", f(food.exhaustionLevel));
			o.addProperty("jumpTrigger", p.jumpTriggerTime);
			o.addProperty("flying", b(p.getAbilities().flying));
			o.addProperty("crouching", b(p.isCrouching()));
		}
		return o;
	}

	public static JsonArray effects(LivingEntity e) {
		JsonArray arr = new JsonArray();
		for (MobEffectInstance m : e.getActiveEffects()) {
			JsonObject o = new JsonObject();
			o.addProperty("id", BuiltInRegistries.MOB_EFFECT.getKey(m.getEffect().value()).toString());
			o.addProperty("amp", m.getAmplifier());
			o.addProperty("dur", m.getDuration());
			arr.add(o);
		}
		return arr;
	}

	// Projectile flight state: the shared motion fields plus arrow ground contact.
	public static JsonObject projectile(Projectile p) {
		JsonObject o = entity(p);
		o.addProperty("id", p.getId());
		o.addProperty("type", BuiltInRegistries.ENTITY_TYPE.getKey(p.getType()).toString());
		o.addProperty("removed", b(p.isRemoved()));
		o.addProperty("noGravity", b(p.isNoGravity()));
		if (p instanceof AbstractArrow a) {
			o.addProperty("inGround", b(a.isInGround()));
			o.addProperty("life", a.life);
			o.addProperty("shake", a.shakeTime);
			o.addProperty("crit", b(a.isCritArrow()));
			o.addProperty("baseDamage", d(a.baseDamage));
		}
		return o;
	}
}
