package minesim.client;

import com.google.gson.JsonObject;
import minesim.Snapshot;
import minesim.oracle.OracleArena;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageTypes;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.List;
import java.util.Random;

// The oracle scenario library. Coordinates are absolute; the arena floor's top face is at y = -63, so a
// player standing on the base floor has feet at Y0. Yaw 0 faces +Z (south), 90 faces -X (west), -90
// faces +X (east), 180 faces -Z (north). Every scenario is self-contained: the runner clears the arena
// before building the next one.
public final class Scenarios {
	static final int Y0 = -63;

	private Scenarios() {
	}

	public static List<Scenario> all() {
		List<Scenario> s = new ArrayList<>();
		// Ground locomotion.
		s.add(walkBasic());
		s.add(sprintJump());
		s.add(sprintRules());
		s.add(sprintHungry());
		s.add(sneakEdges());
		s.add(crouchTunnel());
		s.add(stepsCourse());
		s.add(wallSlide());
		s.add(ledgeFall());
		// Block physics.
		s.add(iceSlip());
		s.add(slimeBlock());
		s.add(honeySoulSand());
		s.add(bedBounce());
		s.add(cobwebBerries());
		s.add(powderSnow());
		// Climbing.
		s.add(ladderClimb());
		s.add(vinesScaffolding());
		// Status effects.
		s.add(effectSpeed());
		s.add(effectJumpBoost());
		s.add(effectSlowFalling());
		s.add(effectLevitation());
		// Falling and damage.
		s.add(fallDamage(4));
		s.add(fallDamage(8));
		s.add(fallDamage(16));
		s.add(fallOntoHay());
		s.add(fallIntoWater());
		// Fluids.
		s.add(waterPool());
		s.add(waterSwim());
		s.add(waterFlow());
		s.add(lavaPool());
		s.add(bubbleColumns());
		// Knockback and projectiles.
		s.add(knockbackStanding());
		s.add(knockbackMoving());
		s.add(projectileHits());
		s.add(projectileFlights());
		// Long mixed sequences.
		s.add(randomCourse("random_course_a", 1L));
		s.add(randomCourse("random_course_b", 2L));
		s.add(legacyCapture());
		return s;
	}

	// ---------------------------------------------------------------- ground locomotion

	static Scenario walkBasic() {
		return Scenario.of("walk_basic", "Walking in every direction, diagonals, turning while walking, and coming to rest.")
			.start(0.5, Y0, -10.5, 0.0F, 0.0F)
			.hold(10, "")
			.hold(40, "w")
			.hold(20, "a")
			.hold(20, "d")
			.hold(25, "s")
			.hold(30, "wa")
			.hold(30, "wd")
			.hold(15, "")
			.hold(40, "w", 3.0F, 0.0F)
			.hold(30, "wa", -5.5F, 1.0F)
			.hold(20, "sd")
			.hold(20, "");
	}

	static Scenario sprintJump() {
		return Scenario.of("sprint_jump", "Sprinting, sprint-jumping straight and while turning, jumping in place and backwards.")
			.start(0.5, Y0, -18.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(30, "wr")
			.hold(60, "wrj")
			.hold(40, "wrj", -4.0F, 0.0F)
			.hold(20, "")
			.hold(30, "j")
			.hold(25, "sj")
			.hold(20, "wj", 9.0F, 0.0F)
			.hold(20, "");
	}

	static Scenario sprintRules() {
		Scenario s = Scenario.of("sprint_rules", "When sprinting starts and stops: sprint key, double-tapping forward, releasing forward, sneaking, and running into a wall.")
			.fill(-3, Y0, 12, 3, Y0 + 2, 12, "minecraft:stone")
			.start(0.5, Y0, -16.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(15, "w")
			.hold(15, "wr")
			.hold(5, "r")
			.hold(10, "")
			// double tap forward
			.hold(1, "w")
			.hold(2, "")
			.hold(20, "w")
			.hold(10, "")
			// sprint, then sneak while holding sprint
			.hold(15, "wr")
			.hold(15, "wrc")
			.hold(10, "wr")
			// sprint backwards / strafing (not allowed)
			.hold(10, "sr")
			.hold(10, "ar")
			// sprint into the wall at z = 12
			.hold(40, "wr")
			.hold(10, "");
		return s;
	}

	static Scenario sprintHungry() {
		return Scenario.of("sprint_hungry", "With food at 6 the player cannot start sprinting; at 7 it can.")
			.start(0.5, Y0, -16.5, 0.0F, 0.0F)
			.food(6)
			.hold(5, "")
			.hold(30, "wr")
			.hold(10, "")
			.hold(1, "w")
			.hold(2, "")
			.hold(20, "w")
			.hold(10, "");
	}

	static Scenario sneakEdges() {
		Scenario s = Scenario.of("sneak_edges", "Sneaking towards every edge and corner of a raised platform (edge back-off), sneaking off a slab, and jumping off while sneaking.")
			.fill(-1, Y0, -1, 1, Y0 + 1, 1, "minecraft:stone")
			.fill(4, Y0, -1, 6, Y0, 1, "minecraft:stone_slab[type=bottom]")
			.start(0.5, Y0 + 2, 0.5, 0.0F, 0.0F)
			.hold(5, "c");
		float[] yaws = {0.0F, 90.0F, 180.0F, -90.0F, 45.0F, 135.0F, -135.0F, -45.0F, 20.0F};
		for (float yaw : yaws) {
			s.look(yaw, 0.0F).hold(30, "wc").hold(10, "sc");
		}
		s.look(-90.0F, 0.0F).hold(20, "wc").hold(25, "wa c").hold(25, "wdc");
		// strafe and back-pedal against the edges
		s.look(0.0F, 0.0F).hold(25, "ac").hold(25, "dc").hold(25, "sc");
		// jump off while sneaking (airborne -> no back-off)
		s.look(180.0F, 0.0F).hold(20, "wcj").hold(20, "");
		return s;
	}

	static Scenario crouchTunnel() {
		return Scenario.of("crouch_tunnel", "Crawling under a 1.5-block ceiling: entering crouched, releasing sneak inside (pose stays crouched), and standing up on exit.")
			.fill(-1, Y0 + 1, 0, -1, Y0 + 2, 8, "minecraft:stone")
			.fill(1, Y0 + 1, 0, 1, Y0 + 2, 8, "minecraft:stone")
			.fill(0, Y0 + 1, 0, 0, Y0 + 1, 8, "minecraft:stone_slab[type=top]")
			.start(0.5, Y0, -3.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(15, "w")
			.hold(15, "wc")
			.hold(40, "w")
			.hold(20, "wr")
			.hold(30, "w")
			.hold(10, "");
	}

	static Scenario stepsCourse() {
		Scenario s = Scenario.of("steps_course", "Walking and sprinting over slabs, stairs, carpet, snow layers, a path block, soul sand, farmland, a full block (jumped), a fence (blocked) and a wall.");
		int z = -14;
		s.fill(-2, Y0, z, 2, Y0, z, "minecraft:stone_slab[type=bottom]");
		s.fill(-2, Y0, z + 2, 2, Y0, z + 2, "minecraft:oak_stairs[facing=south,half=bottom,shape=straight]");
		s.fill(-2, Y0 + 1, z + 3, 2, Y0 + 1, z + 3, "minecraft:white_carpet");
		s.fill(-2, Y0, z + 3, 2, Y0, z + 3, "minecraft:stone");
		s.fill(-2, Y0, z + 5, 2, Y0, z + 5, "minecraft:snow[layers=3]");
		s.fill(-2, Y0, z + 6, 2, Y0, z + 6, "minecraft:snow[layers=6]");
		s.fill(-2, Y0, z + 8, 2, Y0, z + 8, "minecraft:dirt_path");
		s.fill(-2, Y0, z + 10, 2, Y0, z + 10, "minecraft:soul_sand");
		s.fill(-2, Y0, z + 12, 2, Y0, z + 12, "minecraft:farmland[moisture=0]");
		s.fill(-2, Y0, z + 14, 2, Y0, z + 14, "minecraft:stone");
		s.fill(-2, Y0, z + 18, 2, Y0, z + 18, "minecraft:oak_fence[east=true,west=true,north=false,south=false,waterlogged=false]");
		s.fill(-2, Y0, z + 21, 2, Y0, z + 21, "minecraft:cobblestone_wall[east=low,west=low,north=none,south=none,up=true,waterlogged=false]");
		s.start(0.5, Y0, -17.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(70, "w")
			.hold(5, "wj")
			.hold(30, "w")
			.hold(15, "")
			.look(180.0F, 0.0F)
			.hold(60, "wr")
			.hold(40, "wrj")
			.hold(10, "")
			.look(0.0F, 0.0F)
			.hold(50, "wrj", 0.5F, 0.0F)
			.hold(20, "");
		return s;
	}

	static Scenario wallSlide() {
		return Scenario.of("wall_slide", "Sliding along walls at shallow angles, into corners, along glass panes, fences and iron bars, and into a closed door and trapdoors.")
			.fill(3, Y0, -20, 3, Y0 + 2, 20, "minecraft:stone")
			.fill(-20, Y0, 15, 3, Y0 + 2, 15, "minecraft:stone")
			.fill(-3, Y0, -20, -3, Y0, -2, "minecraft:glass_pane[north=true,south=true,east=false,west=false,waterlogged=false]")
			.fill(-3, Y0, 0, -3, Y0, 6, "minecraft:oak_fence[north=true,south=true,east=false,west=false,waterlogged=false]")
			.fill(-3, Y0, 8, -3, Y0, 13, "minecraft:iron_bars[north=true,south=true,east=false,west=false,waterlogged=false]")
			.block(0, Y0, 10, "minecraft:oak_door[facing=north,half=lower,hinge=left,open=false,powered=false]")
			.block(0, Y0 + 1, 10, "minecraft:oak_door[facing=north,half=upper,hinge=left,open=false,powered=false]")
			.block(1, Y0, 10, "minecraft:oak_trapdoor[facing=north,half=bottom,open=true,powered=false,waterlogged=false]")
			.block(-1, Y0, 10, "minecraft:oak_trapdoor[facing=north,half=top,open=false,powered=false,waterlogged=false]")
			.start(1.5, Y0, -18.5, 0.0F, 0.0F)
			.hold(5, "")
			.look(-10.0F, 0.0F)
			.hold(40, "wr")
			.hold(20, "wrj")
			.hold(15, "w")
			.look(30.0F, 0.0F)
			.hold(35, "w")
			.look(80.0F, 0.0F)
			.hold(20, "wr")
			.look(170.0F, 0.0F)
			.hold(40, "wr")
			.look(-45.0F, 0.0F)
			.hold(30, "wrj")
			.look(0.0F, 0.0F)
			.hold(10, "");
	}

	static Scenario ledgeFall() {
		return Scenario.of("ledge_fall", "Walking off ledges of different heights, jumping down a staircase, and falling while moving.")
			.fill(-2, Y0, -12, 2, Y0 + 2, -4, "minecraft:stone")
			.fill(-2, Y0 + 3, -12, 2, Y0 + 3, -10, "minecraft:stone")
			.fill(-2, Y0, -3, 2, Y0, -3, "minecraft:stone")
			.fill(-2, Y0 + 1, -3, 2, Y0 + 1, -3, "minecraft:stone_slab[type=bottom]")
			.fill(-2, Y0, -2, 2, Y0, -2, "minecraft:stone_slab[type=bottom]")
			.start(0.5, Y0 + 4, -11.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(30, "w")
			.hold(40, "w")
			.hold(30, "wj")
			.hold(15, "")
			.look(180.0F, 0.0F)
			.hold(20, "wrj")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- block physics

	static Scenario iceSlip() {
		return Scenario.of("ice_slip", "Walking, sprinting and jumping across ice, packed ice and blue ice floors, then letting go and sliding.")
			.fill(-3, -64, -20, 3, -64, -9, "minecraft:ice")
			.fill(-3, -64, -8, 3, -64, 3, "minecraft:packed_ice")
			.fill(-3, -64, 4, 3, -64, 20, "minecraft:blue_ice")
			.start(0.5, Y0, -19.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(30, "w")
			.hold(30, "wrj")
			.hold(30, "")
			.hold(25, "wr")
			.hold(40, "")
			.look(180.0F, 0.0F)
			.hold(20, "wa")
			.hold(40, "");
	}

	static Scenario slimeBlock() {
		return Scenario.of("slime_block", "Walking on slime (step slow-down), jumping on it, falling onto it from height (bounces), and sneak-landing (no bounce).")
			.fill(-4, -64, -6, 4, -64, 20, "minecraft:slime_block")
			.fill(-1, Y0, -12, 1, Y0 + 4, -10, "minecraft:stone")
			.start(0.5, Y0 + 5, -10.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(10, "w")
			.hold(80, "")
			.hold(30, "w")
			.hold(30, "wj")
			.hold(20, "j")
			.hold(20, "")
			.hold(5, "")
			.now((level, player, log) -> {
				player.teleportTo(level, 0.5, Y0 + 5.0, 5.5, java.util.Set.of(), 0.0F, 0.0F, true);
				log.addProperty("teleport", 1);
			})
			.hold(10, "")
			.hold(40, "c")
			.hold(20, "");
	}

	static Scenario honeySoulSand() {
		return Scenario.of("honey_soul_sand", "Speed and jump factors: honey (slow, low jump, wall slide) and soul sand (slow, sunken shape).")
			.fill(-3, -64, -20, 3, -64, -4, "minecraft:honey_block")
			.fill(-3, -64, -3, 3, -64, 12, "minecraft:soul_sand")
			.fill(-3, Y0, 16, 3, Y0 + 5, 16, "minecraft:honey_block")
			.fill(-3, Y0, 14, 3, Y0 + 1, 14, "minecraft:stone")
			.start(0.5, Y0, -19.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "w")
			.hold(30, "wrj")
			.hold(40, "wr")
			.hold(40, "wrj")
			.hold(10, "")
			.hold(20, "wj")
			.hold(40, "w")
			.hold(20, "");
	}

	static Scenario bedBounce() {
		return Scenario.of("bed_bounce", "Falling onto a bed (bounce) from a 5-block tower.")
			.fill(-1, Y0, -3, 1, Y0 + 4, -1, "minecraft:stone")
			.block(0, Y0, 1, "minecraft:red_bed[facing=south,part=foot,occupied=false]")
			.block(0, Y0, 2, "minecraft:red_bed[facing=south,part=head,occupied=false]")
			.start(0.5, Y0 + 5, -1.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(8, "w")
			.hold(60, "")
			.hold(10, "j")
			.hold(20, "");
	}

	static Scenario cobwebBerries() {
		return Scenario.of("cobweb_berries", "Walking into and falling into cobwebs, and walking through a grown sweet berry bush (slowed and hurt).")
			.fill(-1, Y0, -6, 1, Y0 + 1, -6, "minecraft:cobweb")
			.fill(-1, Y0, 0, 1, Y0 + 3, 0, "minecraft:stone")
			.block(0, Y0 + 1, 2, "minecraft:cobweb")
			.block(0, Y0, 2, "minecraft:cobweb")
			.fill(-1, Y0, 8, 1, Y0, 9, "minecraft:sweet_berry_bush[age=3]")
			.fill(-1, -64, 6, 1, -64, 11, "minecraft:grass_block[snowy=false]")
			.start(0.5, Y0, -9.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "w")
			.hold(20, "wj")
			.hold(10, "")
			.now((level, player, log) -> {
				player.teleportTo(level, 0.5, Y0 + 4.0, 0.5, java.util.Set.of(), 0.0F, 0.0F, true);
				log.addProperty("teleport", 1);
			})
			.hold(10, "")
			.hold(6, "w")
			.hold(60, "")
			.hold(40, "w")
			.hold(20, "wr")
			.hold(20, "");
	}

	static Scenario powderSnow() {
		return Scenario.of("powder_snow", "Walking into powder snow without leather boots: sinking, being slowed, jumping to climb out.")
			.fill(-2, Y0, -2, 2, Y0 + 1, 4, "minecraft:powder_snow")
			.start(0.5, Y0, -6.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "w")
			.hold(40, "wj")
			.hold(20, "j")
			.hold(30, "")
			.hold(30, "wj")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- climbing

	static Scenario ladderClimb() {
		return Scenario.of("ladder_climb", "Climbing a ladder by walking into it, by jumping, sliding down, holding with sneak, and stepping off at the top.")
			.fill(-2, Y0, 4, 2, Y0 + 6, 5, "minecraft:stone")
			.fill(0, Y0, 3, 0, Y0 + 6, 3, "minecraft:ladder[facing=north,waterlogged=false]")
			.start(0.5, Y0, -0.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(50, "w")
			.hold(30, "")
			.hold(20, "wc")
			.hold(25, "c")
			.hold(20, "")
			.hold(30, "j")
			.hold(15, "")
			.hold(80, "w")
			.hold(20, "w")
			.hold(15, "");
	}

	static Scenario vinesScaffolding() {
		return Scenario.of("vines_scaffolding", "Climbing vines on a wall, a twisting-vine column, and a scaffolding tower (standing on top, sneaking down through it).")
			.fill(-6, Y0, 4, -4, Y0 + 6, 4, "minecraft:stone")
			.fill(-5, Y0, 3, -5, Y0 + 6, 3, "minecraft:vine[north=false,east=false,south=true,west=false,up=false]")
			.fill(0, Y0, 0, 0, Y0 + 4, 0, "minecraft:twisting_vines_plant")
			.block(0, Y0 + 5, 0, "minecraft:twisting_vines[age=25]")
			.fill(5, Y0, 0, 5, Y0 + 4, 0, "minecraft:scaffolding[bottom=false,distance=0,waterlogged=false]")
			.start(-4.5, Y0, -1.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(60, "w")
			.hold(20, "")
			.hold(20, "c")
			.now((level, player, log) -> {
				player.teleportTo(level, 0.5, Y0, -1.5, java.util.Set.of(), 0.0F, 0.0F, true);
				log.addProperty("teleport", 1);
			})
			.hold(10, "")
			.hold(40, "wj")
			.hold(30, "")
			.now((level, player, log) -> {
				player.teleportTo(level, 5.5, Y0, -1.5, java.util.Set.of(), 0.0F, 0.0F, true);
				log.addProperty("teleport", 1);
			})
			.hold(10, "")
			.hold(15, "w")
			.hold(60, "j")
			.hold(20, "")
			.hold(40, "c")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- status effects

	static Scenario effectSpeed() {
		return Scenario.of("effect_speed", "Speed II, then slowness I, then both at once, while walking, sprinting and sprint-jumping.")
			.effect("minecraft:speed", 1)
			.start(0.5, Y0, -20.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(25, "w")
			.hold(30, "wrj")
			.hold(10, "")
			.now((level, player, log) -> {
				player.removeAllEffects();
				OracleArena.addEffect(player, "minecraft:slowness", 0, 100000);
			})
			.hold(15, "")
			.look(180.0F, 0.0F)
			.hold(25, "w")
			.hold(30, "wrj")
			.hold(10, "")
			.now((level, player, log) -> OracleArena.addEffect(player, "minecraft:speed", 2, 100000))
			.hold(15, "")
			.look(0.0F, 0.0F)
			.hold(25, "wr")
			.hold(30, "wrj")
			.hold(10, "");
	}

	static Scenario effectJumpBoost() {
		return Scenario.of("effect_jump_boost", "Jump boost II and V: jumping in place, sprint-jumping, and landing on a raised block.")
			.effect("minecraft:jump_boost", 1)
			.fill(-2, Y0, 4, 2, Y0 + 1, 6, "minecraft:stone")
			.start(0.5, Y0, -14.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "j")
			.hold(40, "wrj")
			.hold(20, "")
			.now((level, player, log) -> {
				player.removeAllEffects();
				OracleArena.addEffect(player, "minecraft:jump_boost", 4, 100000);
			})
			.hold(10, "")
			.look(180.0F, 0.0F)
			.hold(40, "j")
			.hold(40, "wj")
			.hold(20, "");
	}

	static Scenario effectSlowFalling() {
		return Scenario.of("effect_slow_falling", "Slow falling: stepping off an 8-block tower, jumping and sprint-jumping.")
			.effect("minecraft:slow_falling", 0)
			.fill(-1, Y0, -13, 1, Y0 + 7, -11, "minecraft:stone")
			.start(0.5, Y0 + 8, -11.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(10, "w")
			.hold(90, "")
			.hold(30, "j")
			.hold(40, "wrj")
			.hold(20, "");
	}

	static Scenario effectLevitation() {
		return Scenario.of("effect_levitation", "Levitation I lifting the player, moving while levitating, then the effect removed mid-air and the fall.")
			.effect("minecraft:levitation", 0)
			.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.settle(2)
			.hold(40, "")
			.hold(20, "w")
			.hold(10, "")
			.now((level, player, log) -> player.removeAllEffects())
			.hold(60, "")
			.hold(10, "");
	}

	// ---------------------------------------------------------------- falling and damage

	static Scenario fallDamage(int height) {
		return Scenario.of("fall_damage_" + height, "Walking off a " + height + "-block tower onto stone: fall distance and fall damage.")
			.fill(-1, Y0, -3, 1, Y0 + height - 1, -1, "minecraft:stone")
			.start(0.5, Y0 + height, -1.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(6, "w")
			.hold(40 + height * 3, "")
			.hold(10, "");
	}

	static Scenario fallOntoHay() {
		return Scenario.of("fall_onto_hay", "A 10-block fall onto a hay bale (reduced damage).")
			.fill(-1, Y0, -3, 1, Y0 + 9, -1, "minecraft:stone")
			.fill(-1, Y0, 0, 1, Y0, 2, "minecraft:hay_block[axis=y]")
			.start(0.5, Y0 + 10, -1.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(6, "w")
			.hold(60, "")
			.hold(10, "");
	}

	static Scenario fallIntoWater() {
		return Scenario.of("fall_into_water", "A 12-block fall into a 2-deep pool (no damage), then swimming out.")
			.fill(-3, Y0, -3, 3, Y0 + 11, -2, "minecraft:stone")
			.fill(-3, Y0, -1, -3, Y0 + 1, 5, "minecraft:stone")
			.fill(3, Y0, -1, 3, Y0 + 1, 5, "minecraft:stone")
			.fill(-2, Y0, 5, 2, Y0 + 1, 5, "minecraft:stone")
			.fill(-2, Y0, -1, 2, Y0 + 1, 4, "minecraft:water[level=0]")
			.start(0.5, Y0 + 12, -2.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(6, "w")
			.hold(60, "")
			.hold(40, "wj")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- fluids

	static Scenario waterPool() {
		return Scenario.of("water_pool", "Walking into a 3-deep pool, sinking, swimming up with jump, sneaking down, and jumping out over the rim.")
			.fill(-4, -64, -4, 4, Y0 + 2, 4, "minecraft:stone")
			.fill(-3, Y0, -3, 3, Y0 + 2, 3, "minecraft:water[level=0]")
			.start(0.5, Y0 + 3, -3.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(20, "w")
			.hold(40, "")
			.hold(40, "j")
			.hold(30, "c")
			.hold(30, "")
			.hold(30, "wj")
			.hold(30, "w")
			.hold(20, "");
	}

	static Scenario waterSwim() {
		return Scenario.of("water_swim", "Sprint-swimming underwater (swimming pose) in a deep pool, steering up and down with pitch, surfacing.")
			.fill(-6, -64, -12, 6, Y0 + 4, 12, "minecraft:stone")
			.fill(-5, Y0, -11, 5, Y0 + 4, 11, "minecraft:water[level=0]")
			.start(0.5, Y0 + 1, -9.5, 0.0F, 10.0F)
			.hold(10, "")
			.hold(30, "wr")
			.hold(20, "wr", 0.0F, 2.0F)
			.hold(20, "wr", 0.0F, -4.0F)
			.hold(30, "wr")
			.look(180.0F, 30.0F)
			.hold(30, "wr")
			.hold(20, "w")
			.hold(20, "j")
			.hold(20, "");
	}

	static Scenario waterFlow() {
		return Scenario.of("water_flow", "A water source on a ledge spreading across the floor; standing and walking in the current, and walking upstream.")
			.fill(-2, Y0, 8, 2, Y0, 9, "minecraft:stone")
			.block(0, Y0 + 1, 8, "minecraft:water[level=0]")
			.start(0.5, Y0, 2.5, 0.0F, 0.0F)
			.settle(120)
			.hold(40, "")
			.hold(30, "w")
			.hold(20, "")
			.hold(20, "a")
			.hold(30, "s")
			.hold(20, "");
	}

	static Scenario lavaPool() {
		return Scenario.of("lava_pool", "Walking into a 2-deep lava pool with fire resistance, sinking, jumping, and climbing out.")
			.effect("minecraft:fire_resistance", 0)
			.fill(-4, -64, -4, 4, Y0 + 1, 4, "minecraft:stone")
			.fill(-3, Y0, -3, 3, Y0 + 1, 3, "minecraft:lava[level=0]")
			.start(0.5, Y0 + 2, -3.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(15, "w")
			.hold(40, "")
			.hold(40, "j")
			.hold(40, "wj")
			.hold(20, "");
	}

	static Scenario bubbleColumns() {
		return Scenario.of("bubble_columns", "An upward bubble column over soul sand and a downward one over magma, entered from above and from the side.")
			.fill(-4, -64, -2, 4, Y0 + 5, 2, "minecraft:stone")
			.block(-2, -64, 0, "minecraft:soul_sand")
			.fill(-2, Y0, 0, -2, Y0 + 5, 0, "minecraft:bubble_column[drag=false]")
			.block(2, -64, 0, "minecraft:magma_block")
			.fill(2, Y0, 0, 2, Y0 + 5, 0, "minecraft:bubble_column[drag=true]")
			.start(-0.5, Y0 + 6, 0.5, 90.0F, 0.0F)
			.hold(5, "")
			.hold(5, "w")
			.hold(60, "")
			.now((level, player, log) -> {
				player.teleportTo(level, 2.5, Y0 + 7.0, 0.5, java.util.Set.of(), 90.0F, 0.0F, true);
				log.addProperty("teleport", 1);
			})
			.hold(10, "")
			.hold(60, "")
			.hold(40, "j")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- knockback and projectiles

	static Scenario knockbackStanding() {
		return Scenario.of("knockback_standing", "Damage with knockback while standing still: a hit, a weaker hit inside the invulnerability window, a stronger one inside it, and a hit after it ends.")
			.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.hold(10, "")
			.now(hurt(2.0F, 0.5, 3.5))
			.hold(5, "")
			.now(hurt(1.0F, -2.5, 0.5))
			.hold(5, "")
			.now(hurt(4.0F, 3.5, 0.5))
			.hold(30, "")
			.now(hurt(1.0F, 0.5, -3.5))
			.hold(40, "");
	}

	static Scenario knockbackMoving() {
		return Scenario.of("knockback_moving", "Knockback while walking, sprint-jumping and airborne, applied directly with the vanilla knockback strength.")
			.start(0.5, Y0, -18.5, 0.0F, 0.0F)
			.hold(10, "")
			.hold(10, "w")
			.now(knock(0.4, 0.0, -1.0))
			.hold(30, "w")
			.hold(10, "wrj")
			.now(knock(0.4, 1.0, 0.3))
			.hold(30, "wrj")
			.hold(3, "j")
			.now(knock(0.8, -0.5, 0.5))
			.hold(40, "")
			.now(hurt(3.0F, 0.5, 0.5))
			.hold(30, "");
	}

	static Scenario projectileHits() {
		return Scenario.of("projectile_hits", "A snowball, an egg and an arrow fired at the standing player: flight, impact, damage and knockback.")
			.trackProjectiles()
			.start(0.5, Y0, 0.5, 180.0F, 0.0F)
			.hold(10, "")
			.now(spawn("minecraft:snowball", 0.5, Y0 + 1.5, 8.5, 0.0, 0.1, -1.0))
			.hold(40, "")
			.now(spawn("minecraft:egg", 6.5, Y0 + 1.2, 0.5, -1.2, 0.15, 0.0))
			.hold(40, "")
			.now(spawn("minecraft:arrow", 0.5, Y0 + 1.6, -9.5, 0.0, 0.15, 2.0))
			.hold(60, "");
	}

	static Scenario projectileFlights() {
		Scenario s = Scenario.of("projectile_flights", "Projectiles flown through the empty arena, into walls, slabs, stairs, a fence, glass panes and a water pool; arrows sticking into blocks.")
			.trackProjectiles()
			.fill(-20, Y0, 12, 20, Y0 + 6, 12, "minecraft:stone")
			.fill(-10, Y0, 6, -6, Y0 + 2, 6, "minecraft:stone_slab[type=bottom]")
			.fill(-4, Y0, 6, 0, Y0, 6, "minecraft:oak_stairs[facing=north,half=bottom,shape=straight]")
			.fill(2, Y0, 6, 6, Y0 + 2, 6, "minecraft:oak_fence[east=true,west=true,north=false,south=false,waterlogged=false]")
			.fill(8, Y0, 6, 12, Y0 + 2, 6, "minecraft:glass_pane[east=true,west=true,north=false,south=false,waterlogged=false]")
			.fill(-16, -64, -16, -8, Y0 + 2, -8, "minecraft:stone")
			.fill(-15, Y0, -15, -9, Y0 + 2, -9, "minecraft:water[level=0]")
			.start(18.5, Y0, -18.5, 0.0F, 0.0F)
			.hold(10, "");
		String[] types = {"minecraft:snowball", "minecraft:egg", "minecraft:arrow", "minecraft:ender_pearl", "minecraft:spectral_arrow"};
		double[][] shots = {
			// x, y, z, dx, dy, dz
			{-8.0, -60.0, -2.0, 0.0, 0.2, 1.5},
			{-2.0, -61.0, -2.0, 0.0, 0.1, 1.2},
			{4.0, -61.5, -2.0, 0.05, 0.05, 1.6},
			{10.0, -61.0, -2.0, 0.0, 0.1, 2.5},
			{0.3, -50.0, -6.0, 0.4, 0.8, 0.9},
			{-12.0, -55.0, -12.0, 0.0, -0.5, 0.0},
			{-18.0, -58.0, -20.0, 0.6, 0.3, 0.6},
		};
		int k = 0;
		for (double[] shot : shots) {
			for (String type : types) {
				double off = (k % 5) * 0.37;
				s.now(spawn(type, shot[0] + off, shot[1], shot[2], shot[3], shot[4], shot[5]));
				s.hold(3, "");
				k++;
			}
			s.hold(30, "");
		}
		s.hold(60, "");
		return s;
	}

	// ---------------------------------------------------------------- long mixed sequences

	// A varied obstacle course crossed with seeded pseudo-random inputs: long free-running stretches
	// where every mechanic interacts.
	static Scenario randomCourse(String name, long seed) {
		Scenario s = Scenario.of(name, "Seeded random inputs across a mixed obstacle course (slabs, stairs, ice, slime, a ladder, water, honey, fences, walls).")
			.fill(-20, -64, 8, -12, -64, 20, "minecraft:ice")
			.fill(12, -64, 8, 20, -64, 20, "minecraft:slime_block")
			.fill(-4, Y0, 10, 4, Y0, 12, "minecraft:stone_slab[type=bottom]")
			.fill(-4, Y0, 14, 4, Y0, 14, "minecraft:oak_stairs[facing=south,half=bottom,shape=straight]")
			.fill(-4, Y0, 15, 4, Y0 + 1, 18, "minecraft:stone")
			.fill(0, Y0 + 2, 18, 0, Y0 + 2, 18, "minecraft:ladder[facing=north,waterlogged=false]")
			.fill(-20, -64, -20, -10, Y0, -10, "minecraft:stone")
			.fill(-19, Y0, -19, -11, Y0, -11, "minecraft:water[level=0]")
			.fill(10, -64, -20, 20, -64, -10, "minecraft:honey_block")
			.fill(-8, Y0, -4, -8, Y0, 4, "minecraft:oak_fence[north=true,south=true,east=false,west=false,waterlogged=false]")
			.fill(8, Y0, -4, 8, Y0 + 1, 4, "minecraft:stone")
			.fill(-2, Y0, -8, 2, Y0, -8, "minecraft:snow[layers=4]")
			.start(0.5, Y0, 0.5, 0.0F, 0.0F);
		Random r = new Random(seed);
		String[] moves = {"w", "w", "wr", "wrj", "wj", "wa", "wd", "a", "d", "s", "", "j", "wc", "c", "wrj", "wr"};
		for (int i = 0; i < 40; i++) {
			String keys = moves[r.nextInt(moves.length)];
			int len = 5 + r.nextInt(20);
			float turn = (r.nextFloat() - 0.5F) * 12.0F;
			s.hold(len, keys, turn, 0.0F);
		}
		s.hold(10, "");
		return s;
	}

	// The routine the original capture script drove: walk, sprint, sprint-jump, sneak, turn, under
	// speed / jump boost / slow falling / slowness, then a knockback.
	static Scenario legacyCapture() {
		return Scenario.of("legacy_capture", "The original capture routine: walking, sprinting, sprint-jumping, sneaking and turning under changing effects, then a knockback.")
			.start(0.5, Y0, -20.5, 0.0F, 0.0F)
			.hold(10, "")
			.now((level, player, log) -> OracleArena.addEffect(player, "minecraft:speed", 1, 1200))
			.hold(30, "w")
			.hold(40, "wr")
			.now((level, player, log) -> OracleArena.addEffect(player, "minecraft:jump_boost", 1, 1200))
			.hold(60, "wrj")
			.now((level, player, log) -> OracleArena.addEffect(player, "minecraft:slow_falling", 0, 800))
			.hold(40, "wrj")
			.now((level, player, log) -> OracleArena.addEffect(player, "minecraft:slowness", 0, 600))
			.hold(30, "wc")
			.hold(30, "wr", 2.0F, 0.0F)
			.now((level, player, log) -> player.removeAllEffects())
			.hold(10, "")
			.now(hurt(1.0F, 0.5, -19.0))
			.hold(30, "");
	}

	// ---------------------------------------------------------------- server actions

	// The server-side player state that knockback and damage read and write.
	static JsonObject serverState(net.minecraft.server.level.ServerPlayer p) {
		JsonObject o = new JsonObject();
		Vec3 v = p.getDeltaMovement();
		o.addProperty("dx", Snapshot.d(v.x));
		o.addProperty("dy", Snapshot.d(v.y));
		o.addProperty("dz", Snapshot.d(v.z));
		o.addProperty("x", Snapshot.d(p.getX()));
		o.addProperty("y", Snapshot.d(p.getY()));
		o.addProperty("z", Snapshot.d(p.getZ()));
		o.addProperty("ground", Snapshot.b(p.onGround()));
		o.addProperty("health", Snapshot.f(p.getHealth()));
		o.addProperty("invul", p.invulnerableTime);
		o.addProperty("hurtTime", p.hurtTime);
		o.addProperty("lastHurt", Snapshot.f(p.lastHurt));
		return o;
	}

	// Generic mob-attack damage from a point source at (sx, feet y, sz): vanilla damage handling with
	// its standard 0.4 knockback away from the source.
	static Scenario.ServerAction hurt(float amount, double sx, double sz) {
		return (level, player, log) -> {
			log.addProperty("op", "hurt");
			log.addProperty("amount", Snapshot.f(amount));
			log.addProperty("sx", Snapshot.d(sx));
			log.addProperty("sz", Snapshot.d(sz));
			log.add("before", serverState(player));
			DamageSource src = new DamageSource(
				level.registryAccess().lookupOrThrow(Registries.DAMAGE_TYPE).getOrThrow(DamageTypes.MOB_ATTACK),
				new Vec3(sx, player.getY(), sz));
			boolean ok = player.hurtServer(level, src, amount);
			log.addProperty("result", Snapshot.b(ok));
			log.add("after", serverState(player));
		};
	}

	// A bare LivingEntity.knockback on the server player, synced to the client like a hit would be.
	static Scenario.ServerAction knock(double strength, double dx, double dz) {
		return (level, player, log) -> {
			log.addProperty("op", "knockback");
			log.addProperty("strength", Snapshot.d(strength));
			log.addProperty("kx", Snapshot.d(dx));
			log.addProperty("kz", Snapshot.d(dz));
			log.add("before", serverState(player));
			player.knockback(strength, dx, dz);
			player.hurtMarked = true;
			log.add("after", serverState(player));
		};
	}

	static Scenario.ServerAction spawn(String type, double x, double y, double z, double dx, double dy, double dz) {
		return (level, player, log) -> {
			EntityType<?> t = EntityType.byString(type).orElseThrow(() -> new IllegalArgumentException(type));
			Entity e = t.create(level, EntitySpawnReason.COMMAND);
			if (e == null) {
				throw new IllegalStateException("could not create " + type);
			}
			e.snapTo(x, y, z, 0.0F, 0.0F);
			e.setDeltaMovement(dx, dy, dz);
			level.addFreshEntity(e);
			log.addProperty("op", "spawn");
			log.addProperty("type", type);
			log.addProperty("id", e.getId());
			log.add("entity", Snapshot.entity(e));
		};
	}
}
