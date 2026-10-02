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
		// Second wave: the paths the first batch leaves untested.
		s.addAll(wave2());
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

	// ================================================================ wave 2
	//
	// A second batch aimed at the code paths the first 38 scenarios leave untested: bed bounces,
	// the open-trapdoor-above-ladder rule, honey wall slides, scaffolding, crawling, more slime,
	// fall-damage edges, knockback in unusual states, fluids in unusual shapes, lava, more effects,
	// the long tail of collision shapes (including the position-offset ones), projectiles and three
	// more seeded courses. Conventions are those of the first batch: absolute coordinates, the
	// arena box is x, z in [-24, 24] and y in [-64, -24], and a player standing on the base floor
	// has its feet at Y0. Several scenarios teleport the player between independent "lanes" with
	// the server action `tp`, so one recording covers many starting situations.

	static List<Scenario> wave2() {
		List<Scenario> s = new ArrayList<>();
		// Beds, climbables, honey, scaffolding, crawling, slime.
		s.add(bedFall("bed_fall_3", 3));
		s.add(bedFall("bed_fall_5", 5));
		s.add(bedFall("bed_fall_8", 8));
		s.add(bedSneakLand());
		s.add(trapdoorLadder());
		s.add(honeyWallSlide());
		s.add(scaffoldingPlate());
		s.add(scaffoldingColumn());
		s.add(scaffoldingUnstable());
		s.add(crawlStartInside());
		s.add(crawlCeilingMix());
		s.add(slimeBounce10());
		s.add(slimeJumpBoost());
		s.add(slimeWalkOff());
		s.add(slimeSneakLand());
		// Fall-damage edges, knockback in unusual states, more effects.
		s.add(fallEdges());
		s.add(fallEdgesB());
		s.add(fallHayJumpBoost());
		s.add(fallSlowFalling());
		s.add(knockbackLadder());
		s.add(knockbackWater());
		s.add(knockbackLedgeSneak());
		s.add(knockbackSprintJump());
		s.add(effectLevitation2());
		s.add(effectSlowness4());
		s.add(effectSpeedIce());
		s.add(effectJumpHoney());
		s.add(effectSlowfallSprintJump());
		// Fluids in unusual shapes and lava.
		s.add(waterWaterfall());
		s.add(waterStairsFlow());
		s.add(waterloggedSlabsStairs());
		s.add(waterloggedMisc());
		s.add(waterPlants());
		s.add(waterExitSprint());
		s.add(waterTunnel());
		s.add(dolphinsGrace());
		s.add(lavaUnresisted());
		s.add(lavaFlow());
		s.add(lavaLanes());
		s.add(waterClimbables());
		s.add(effectsInWater());
		s.add(sprintStarve());
		s.add(powderSnowFreeze());
		// Collision shapes, including the position-offset ones.
		s.add(shapesStairsUp());
		s.add(shapesStairsDown());
		s.add(shapesSlabCorners());
		s.add(shapesContainers());
		s.add(shapesSmallA());
		s.add(shapesSmallB());
		s.add(shapesFencesWalls());
		s.add(shapesBamboo());
		s.add(shapesDripstone());
		s.add(sneakEdgeDrops());
		// Projectiles.
		s.add(projArrowWalking());
		s.add(projArrowSteep());
		s.add(projSnowEggMoving());
		s.add(projArrowWater());
		s.add(projEnderPearl());
		// Long seeded courses.
		s.add(courseC());
		s.add(courseD());
		s.add(courseE());
		return s;
	}

	// ---------------------------------------------------------------- wave 2 helpers

	// Teleport the server player (and so the client) without touching its other state.
	static Scenario.ServerAction tp(double x, double y, double z, float yaw, float pitch) {
		return (level, player, log) -> {
			player.teleportTo(level, x, y, z, java.util.Set.of(), yaw, pitch, true);
			player.resetFallDistance();
			log.addProperty("teleport", 1);
		};
	}

	// Teleport the (standing) player to (x, y, z) facing `yaw`, then let the packet land and the player settle.
	static Scenario place(Scenario s, double x, double y, double z, float yaw) {
		return s.look(yaw, 0.0F).now(tp(x, y, z, yaw, 0.0F)).hold(3, "");
	}

	static final String TRAPDOOR_OPEN_N = "minecraft:oak_trapdoor[facing=north,half=bottom,open=true,powered=false,waterlogged=false]";

	// ---------------------------------------------------------------- beds

	// Three rows of beds (x -1..1, z 0..5, head to the south) behind a tower that ends at z = 0.
	static Scenario bedField(Scenario s, int height) {
		s.fill(-1, Y0, -3, 1, Y0 + height - 1, -1, "minecraft:stone");
		for (int x = -1; x <= 1; x++) {
			for (int z = 0; z <= 4; z += 2) {
				s.block(x, Y0, z, "minecraft:red_bed[facing=south,part=foot,occupied=false]");
				s.block(x, Y0, z + 1, "minecraft:red_bed[facing=south,part=head,occupied=false]");
			}
		}
		return s;
	}

	static Scenario bedFall(String name, int height) {
		Scenario s = Scenario.of(name, "Leaving a " + height + "-block tower and landing on a field of beds: the bounce (66% of the landing speed), halved fall damage, then hopping, walking and sprinting on the beds.");
		bedField(s, height).start(0.5, Y0 + height, -0.4, 0.0F, 0.0F).hold(5, "");
		switch (height) {
			case 3 -> s.hold(12, "w").hold(70, "").hold(20, "w").hold(15, "wrj").hold(30, "");
			case 5 -> s.hold(10, "wr").hold(80, "").hold(15, "j").hold(20, "wr").hold(25, "");
			default -> s.hold(4, "wr").hold(2, "wrj").hold(26, "", 2.0F, 0.0F).hold(70, "").hold(20, "j").hold(10, "");
		}
		return s;
	}

	static Scenario bedSneakLand() {
		Scenario s = Scenario.of("bed_sneak_land", "Walking off a 6-block tower and holding sneak in the air: landing on a bed while sneaking (no bounce, normal fall damage), then sneaking and jumping on the beds.");
		return bedField(s, 6).start(0.5, Y0 + 6, -0.4, 0.0F, 0.0F)
			.hold(5, "")
			.hold(12, "w")
			.hold(30, "wc")
			.hold(20, "c")
			.hold(10, "")
			.hold(15, "j")
			.hold(15, "wc")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- climbables

	static Scenario trapdoorLadder() {
		Scenario s = Scenario.of("trapdoor_ladder", "A ladder topped by an open trapdoor facing the same way (the climb continues into the trapdoor), by one facing another way and by a closed one (neither extends the ladder).");
		int[] xs = {-4, 0, 4};
		String[] top = {
			TRAPDOOR_OPEN_N,
			"minecraft:oak_trapdoor[facing=east,half=bottom,open=true,powered=false,waterlogged=false]",
			"minecraft:oak_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]"};
		for (int i = 0; i < 3; i++) {
			s.fill(xs[i] - 1, Y0, 4, xs[i] + 1, Y0 + 4, 5, "minecraft:stone");
			s.fill(xs[i], Y0, 3, xs[i], Y0 + 3, 3, "minecraft:ladder[facing=north,waterlogged=false]");
			s.block(xs[i], Y0 + 4, 3, top[i]);
		}
		s.start(-3.5, Y0, 0.5, 0.0F, 0.0F).hold(5, "").hold(25, "w").hold(60, "w").hold(20, "");
		for (int i = 1; i < 3; i++) {
			place(s, xs[i] + 0.5, Y0, 0.5, 0.0F).hold(25, "w").hold(60, "w").hold(20, "");
		}
		return s;
	}

	static Scenario honeyWallSlide() {
		Scenario s = Scenario.of("honey_wall_slide", "Sprint-jumping from a 6-block tower against a honey wall and sliding down it, then the same from the other side of the wall.");
		s.fill(-4, Y0, 0, 4, Y0 + 9, 0, "minecraft:honey_block");
		s.fill(-1, Y0, -4, 1, Y0 + 5, -2, "minecraft:stone");
		s.fill(-1, Y0, 3, 1, Y0 + 5, 5, "minecraft:stone");
		s.start(0.5, Y0 + 6, -3.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(14, "wr")
			.hold(12, "wrj")
			.hold(90, "w")
			.hold(15, "");
		place(s, 0.5, Y0 + 6, 4.5, 180.0F)
			.hold(14, "wr")
			.hold(40, "wrj")
			.hold(70, "wj")
			.hold(15, "");
		return s;
	}

	// ---------------------------------------------------------------- scaffolding

	static Scenario scaffoldingPlate() {
		Scenario s = Scenario.of("scaffolding_plate", "A 3x3 scaffolding plate hung from a supported corner column (its cells settle into unstable states, bottom=true with distance 1..2): standing on it, walking off, jumping up through it from below, sneaking down through it.");
		String sc = "minecraft:scaffolding[bottom=false,distance=0,waterlogged=false]";
		s.fill(-1, Y0, -1, -1, Y0 + 1, -1, sc);
		s.fill(-1, Y0 + 1, -1, 1, Y0 + 1, 1, sc);
		s.start(0.5, Y0 + 2, 0.5, 0.0F, 0.0F)
			.hold(10, "")
			.hold(35, "w")
			.hold(20, "");
		place(s, 0.5, Y0, 4.5, 180.0F)
			.hold(18, "w")
			.hold(8, "")
			.hold(50, "j")
			.hold(15, "")
			.hold(50, "c")
			.hold(15, "")
			.hold(30, "wc")
			.hold(10, "");
		return s;
	}

	static Scenario scaffoldingColumn() {
		return Scenario.of("scaffolding_column", "A 6-high scaffolding column on the floor: standing inside it, climbing with jump, standing on top, sneaking down through it (sneak descends in scaffolding), sneak-climbing, and stepping out.")
			.fill(3, Y0, 0, 3, Y0 + 5, 0, "minecraft:scaffolding[bottom=false,distance=0,waterlogged=false]")
			.start(3.5, Y0, 0.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(50, "j")
			.hold(15, "")
			.hold(50, "c")
			.hold(10, "")
			.hold(35, "cj")
			.hold(15, "j")
			.hold(6, "wj")
			.hold(40, "")
			.hold(15, "");
	}

	static Scenario scaffoldingUnstable() {
		Scenario s = Scenario.of("scaffolding_unstable", "A scaffolding cantilever hanging six blocks off a supported column (bottom=true, distance 1..6) with cells stacked on it: walking along its top, falling off the end, jumping up through it from a stone step (landing on the thin bottom plate) and sneaking down through it.");
		String sc = "minecraft:scaffolding[bottom=false,distance=0,waterlogged=false]";
		s.fill(-6, Y0, 0, -6, Y0 + 2, 0, sc);
		s.fill(-5, Y0 + 2, 0, 0, Y0 + 2, 0, sc);
		s.fill(-3, Y0 + 3, 0, -3, Y0 + 3, 0, sc);
		s.fill(-1, Y0 + 3, 0, -1, Y0 + 4, 0, sc);
		s.block(-2, Y0, 0, "minecraft:stone");
		s.block(-4, Y0, 0, "minecraft:stone");
		s.start(-5.5, Y0 + 3, 0.5, -90.0F, 0.0F)
			.hold(10, "")
			.hold(70, "w")
			.hold(25, "");
		place(s, 0.5, Y0, 0.5, 90.0F)
			.hold(9, "w")
			.hold(20, "wj")
			.hold(40, "j")
			.hold(15, "")
			.hold(30, "c");
		place(s, -3.5, Y0 + 3, 0.5, 0.0F)
			.hold(10, "")
			.hold(45, "c")
			.hold(15, "");
		return s;
	}

	// ---------------------------------------------------------------- crawling

	static Scenario crawlStartInside() {
		return Scenario.of("crawl_start_inside", "The player starts inside a 1-block-high tunnel (pose forced to swimming on land): crawling, sprint-crawling, jumping into the ceiling, strafing into the walls, sneaking, turning, crawling out, and sprinting back in.")
			.fill(-1, Y0, 0, -1, Y0 + 1, 3, "minecraft:stone")
			.fill(1, Y0, 0, 1, Y0 + 1, 3, "minecraft:stone")
			.fill(0, Y0 + 1, 0, 0, Y0 + 1, 3, "minecraft:stone")
			.start(0.5, Y0, 1.5, 0.0F, 0.0F)
			.hold(10, "")
			.hold(15, "w")
			.hold(10, "wr")
			.hold(10, "wj")
			.hold(10, "a")
			.hold(10, "d")
			.hold(12, "s")
			.hold(12, "wc")
			.hold(15, "w", 3.0F, 0.0F)
			.look(0.0F, 0.0F)
			.hold(60, "w")
			.hold(20, "")
			.look(180.0F, 0.0F)
			.hold(30, "wr")
			.hold(40, "w");
	}

	static Scenario crawlCeilingMix() {
		Scenario s = Scenario.of("crawl_ceiling_mix", "A tunnel whose ceiling steps up: 0.8125 (top trapdoors), 1.0 (stone), 1.5 (top slab, forced crouch) and 2.0 (stone). Starting inside the lowest part, the pose changes swimming, crouching, standing at the boundaries.");
		s.fill(-1, Y0, 0, -1, Y0 + 2, 7, "minecraft:stone");
		s.fill(1, Y0, 0, 1, Y0 + 2, 7, "minecraft:stone");
		s.fill(0, Y0, 0, 0, Y0, 1, "minecraft:oak_trapdoor[facing=north,half=top,open=false,powered=false,waterlogged=false]");
		s.fill(0, Y0 + 1, 2, 0, Y0 + 1, 3, "minecraft:stone");
		s.fill(0, Y0 + 1, 4, 0, Y0 + 1, 5, "minecraft:stone_slab[type=top,waterlogged=false]");
		s.fill(0, Y0 + 2, 6, 0, Y0 + 2, 7, "minecraft:stone");
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.hold(10, "")
			.hold(100, "w")
			.hold(40, "wc")
			.hold(25, "w")
			.hold(10, "wj")
			.hold(15, "");
		return s;
	}

	// ---------------------------------------------------------------- slime

	static Scenario slimeBounce10() {
		return Scenario.of("slime_bounce_10", "Walking off a 10-block tower onto a slime pad: repeated bounces with decaying height, then walking, sprinting and sprint-jumping on the slime.")
			.fill(-1, Y0, -3, 1, Y0 + 9, -1, "minecraft:stone")
			.fill(-4, -64, 0, 4, -64, 12, "minecraft:slime_block")
			.start(0.5, Y0 + 10, -0.4, 0.0F, 0.0F)
			.hold(5, "")
			.hold(12, "w")
			.hold(160, "")
			.hold(20, "w")
			.hold(25, "wr")
			.hold(40, "wrj")
			.hold(20, "");
	}

	static Scenario slimeJumpBoost() {
		return Scenario.of("slime_jump_boost", "Jump boost III on a slime floor: falling onto it from a tower, then jumping in place and sprint-jumping so jumps and bounces interleave.")
			.effect("minecraft:jump_boost", 2)
			.fill(-1, Y0, -3, 1, Y0 + 5, -1, "minecraft:stone")
			.fill(-5, -64, 0, 5, -64, 14, "minecraft:slime_block")
			.start(0.5, Y0 + 6, -0.4, 0.0F, 0.0F)
			.hold(5, "")
			.hold(12, "w")
			.hold(70, "")
			.hold(50, "j")
			.hold(50, "wrj")
			.hold(40, "")
			.hold(20, "");
	}

	static Scenario slimeWalkOff() {
		Scenario s = Scenario.of("slime_walk_off", "Walking, sprinting, jumping and sneaking across the edge of a slime strip onto stone and back (the block-below friction and the step slow-down change at the edge).")
			.fill(-3, -64, -6, 3, -64, -1, "minecraft:slime_block")
			.start(0.5, Y0, -1.8, 0.0F, 0.0F)
			.hold(5, "")
			.hold(50, "w")
			.hold(10, "");
		place(s, 0.5, Y0, 1.3, 180.0F).hold(35, "wr").hold(15, "");
		place(s, 0.5, Y0, -1.0, 0.0F).hold(10, "wr").hold(12, "wrj").hold(25, "");
		place(s, 0.5, Y0, 1.0, 180.0F).hold(45, "wc").hold(10, "");
		place(s, 0.5, Y0, -0.9, 0.0F).hold(40, "wc").hold(25, "s").hold(15, "");
		return s;
	}

	static Scenario slimeSneakLand() {
		return Scenario.of("slime_sneak_land", "Holding sneak while falling onto slime from 8 blocks (no bounce, full fall damage), sneak-walking and sneak-jumping on it, then a normal landing for contrast.")
			.fill(-1, Y0, -3, 1, Y0 + 7, -1, "minecraft:stone")
			.fill(-4, -64, 0, 4, -64, 12, "minecraft:slime_block")
			.start(0.5, Y0 + 8, -0.4, 0.0F, 0.0F)
			.hold(5, "")
			.hold(12, "w")
			.hold(40, "wc")
			.hold(30, "c")
			.hold(25, "wc")
			.hold(15, "wcj")
			.hold(20, "")
			.now(tp(0.5, Y0 + 8, -0.4, 0.0F, 0.0F))
			.hold(3, "")
			.hold(12, "w")
			.hold(60, "")
			.hold(15, "");
	}

	// ---------------------------------------------------------------- effect helpers

	static Scenario.ServerAction setEffect(String id, int amplifier, int duration) {
		return (level, player, log) -> {
			player.removeAllEffects();
			OracleArena.addEffect(player, id, amplifier, duration);
			log.addProperty("effect", id);
		};
	}

	static Scenario.ServerAction clearEffects() {
		return (level, player, log) -> {
			player.removeAllEffects();
			log.addProperty("effect", "none");
		};
	}

	// ---------------------------------------------------------------- fall-damage edges

	static final String SLAB_BOTTOM = "minecraft:stone_slab[type=bottom,waterlogged=false]";
	static final String TRAPDOOR_BOTTOM_CLOSED = "minecraft:oak_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]";

	static Scenario fallEdges() {
		Scenario s = Scenario.of("fall_edges", "Falls ending just under and just over the whole-number damage thresholds (3.0, 3.0625, 3.5, 3.75, 3.9375, 4.0, 4.125, 4.1875 and a jumped 4.25 blocks) onto stone, carpet, slabs, snow layers and from slab/snow/trapdoor-capped towers.");
		Object[][] lanes = {
			// tower height, cap block, cap collision height, landing block, keys, ticks
			{3, null, 0.0, null, "w", 8},
			{3, "minecraft:white_carpet", 0.0625, null, "wr", 6},
			{3, SLAB_BOTTOM, 0.5, null, "w", 8},
			{4, null, 0.0, "minecraft:white_carpet", "wr", 6},
			{4, null, 0.0, SLAB_BOTTOM, "w", 8},
			{4, null, 0.0, "minecraft:snow[layers=3]", "wr", 6},
			{4, "minecraft:snow[layers=2]", 0.125, "minecraft:snow[layers=1]", "w", 8},
			{4, TRAPDOOR_BOTTOM_CLOSED, 0.1875, null, "wr", 6},
			{3, null, 0.0, null, "wrj", 2}};
		for (int i = 0; i < lanes.length; i++) {
			int x = -20 + 5 * i;
			int h = (Integer) lanes[i][0];
			String cap = (String) lanes[i][1];
			String land = (String) lanes[i][3];
			s.fill(x - 1, Y0, -3, x + 1, Y0 + h - 1, -1, "minecraft:stone");
			if (cap != null) {
				s.fill(x - 1, Y0 + h, -3, x + 1, Y0 + h, -1, cap);
			}
			if (land != null) {
				s.fill(x - 1, Y0, 0, x + 1, Y0, 5, land);
			}
		}
		for (int i = 0; i < lanes.length; i++) {
			int x = -20 + 5 * i;
			double top = Y0 + (Integer) lanes[i][0] + (Double) lanes[i][2];
			if (i == 0) {
				s.start(x + 0.5, top, -0.2, 0.0F, 0.0F).hold(5, "");
			} else {
				place(s, x + 0.5, top, -0.2, 0.0F);
			}
			s.hold((Integer) lanes[i][5], (String) lanes[i][4]).hold(36, "");
		}
		return s;
	}

	static Scenario fallHayJumpBoost() {
		Scenario s = Scenario.of("fall_hay_jump_boost", "Jump boost III raises the safe fall distance to 6: falling 10 and 14 blocks onto hay (damage x0.2) and 9 blocks onto stone.")
			.effect("minecraft:jump_boost", 2);
		int[] xs = {-6, 0, 6};
		int[] hs = {10, 14, 9};
		for (int i = 0; i < 3; i++) {
			s.fill(xs[i] - 1, Y0, -3, xs[i] + 1, Y0 + hs[i] - 1, -1, "minecraft:stone");
			if (i < 2) {
				s.fill(xs[i] - 1, Y0, 0, xs[i] + 1, Y0, 3, "minecraft:hay_block[axis=y]");
			}
		}
		s.start(xs[0] + 0.5, Y0 + hs[0], -0.2, 0.0F, 0.0F).hold(5, "").hold(8, "w").hold(55, "");
		for (int i = 1; i < 3; i++) {
			place(s, xs[i] + 0.5, Y0 + hs[i], -0.2, 0.0F).hold(8, "w").hold(60, "");
		}
		return s;
	}

	static Scenario fallSlowFalling() {
		Scenario s = Scenario.of("fall_slow_falling_stone", "Slow falling off a 12-block tower onto stone (no damage), then the effect running out in mid-air (normal gravity and damage resume).")
			.effect("minecraft:slow_falling", 0)
			.fill(-1, Y0, -3, 1, Y0 + 11, -1, "minecraft:stone")
			.start(0.5, Y0 + 12, -0.4, 0.0F, 0.0F)
			.hold(5, "")
			.hold(10, "w")
			.hold(110, "")
			.hold(10, "wr")
			.hold(10, "");
		s.now(setEffect("minecraft:slow_falling", 0, 50));
		place(s, 0.5, Y0 + 12, -0.4, 0.0F).hold(10, "w").hold(90, "");
		return s;
	}

	// ---------------------------------------------------------------- knockback in unusual states

	static Scenario knockbackLadder() {
		return Scenario.of("knockback_ladder", "Knockback while climbing a ladder: pushed off it, pushed along it, damage from behind the wall, and while holding on with sneak.")
			.fill(-2, Y0, 4, 2, Y0 + 7, 5, "minecraft:stone")
			.fill(0, Y0, 3, 0, Y0 + 7, 3, "minecraft:ladder[facing=north,waterlogged=false]")
			.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(32, "w")
			.hold(12, "w")
			.now(knock(0.5, 0.0, 1.0))
			.hold(30, "w")
			.hold(35, "w")
			.now(hurt(2.0F, 0.5, 6.0))
			.hold(30, "w")
			.hold(20, "c")
			.now(knock(0.4, 1.0, 0.0))
			.hold(30, "c")
			.now(knock(0.4, 0.0, -1.0))
			.hold(30, "w")
			.hold(20, "");
	}

	static Scenario knockbackWater() {
		return Scenario.of("knockback_water", "Knockback and damage while swimming: sprint-swimming, swimming up, at the surface and sinking.")
			.fill(-6, Y0, -9, 6, Y0 + 3, 9, "minecraft:stone")
			.fill(-5, Y0, -8, 5, Y0 + 3, 8, "minecraft:water[level=0]")
			.start(0.5, Y0 + 2, -6.5, 0.0F, 0.0F)
			.settle(60)
			.hold(10, "")
			.hold(15, "wr")
			.now(knock(0.6, 0.0, 1.0))
			.hold(20, "wr")
			.now(hurt(2.0F, 0.5, 6.0))
			.hold(25, "wr")
			.hold(30, "j")
			.now(knock(0.5, -1.0, 0.0))
			.hold(30, "j")
			.hold(20, "")
			.now(hurt(1.0F, -3.0, 0.5))
			.hold(30, "w")
			.now(knock(0.4, 0.5, -1.0))
			.hold(30, "c")
			.hold(20, "");
	}

	static Scenario knockbackLedgeSneak() {
		Scenario s = Scenario.of("knockback_ledge_sneak", "Knockback towards a ledge while sneaking at its edge (a push too weak to lift the player is clamped by the edge back-off, a stronger one lifts it off the ledge), damage while sneaking, then releasing sneak so the push carries the player off.")
			.fill(-2, Y0, -2, 1, Y0 + 2, 1, "minecraft:stone")
			.start(0.5, Y0 + 3, 0.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "wc")
			.now(knock(0.035, 0.0, -1.0))
			.hold(10, "wc")
			.now(knock(0.03, 0.0, -1.0))
			.hold(10, "c")
			.now(hurt(1.0F, 0.5, -3.0))
			.hold(20, "wc")
			.now(knock(0.5, 0.0, -1.0))
			.hold(45, "c")
			.hold(10, "");
		place(s, 0.5, Y0 + 3, 0.5, 90.0F).hold(40, "wc").now(knock(0.7, 1.0, 0.0)).hold(15, "c").hold(5, "").hold(30, "");
		place(s, 0.5, Y0 + 3, 0.5, 0.0F).hold(40, "wc").now(knock(0.04, 0.0, -1.0)).hold(8, "wc").hold(8, "w").hold(40, "");
		return s;
	}

	static Scenario knockbackSprintJump() {
		return Scenario.of("knockback_sprint_jump", "Knockback during a sprint-jump at different phases of the arc (rising, falling, just landed) and damage while airborne.")
			.start(0.5, Y0, -18.5, 0.0F, 0.0F)
			.hold(10, "")
			.hold(10, "wr")
			.hold(6, "wrj")
			.now(knock(0.4, 1.0, 0.3))
			.hold(8, "wrj")
			.now(knock(0.6, -0.5, -1.0))
			.hold(10, "wrj")
			.now(hurt(2.0F, 4.5, -10.0))
			.hold(25, "wrj")
			.now(knock(0.5, 0.0, -1.0))
			.hold(30, "wrj")
			.hold(20, "");
	}

	// ---------------------------------------------------------------- more status effects

	static Scenario effectLevitation2() {
		return Scenario.of("effect_levitation_2", "Levitation II lifting the player (walking and sprinting while it rises), removed in mid-air and the fall, then a short levitation I that runs out.")
			.effect("minecraft:levitation", 1)
			.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.settle(2)
			.hold(30, "")
			.hold(20, "w")
			.hold(20, "wr", 4.0F, 0.0F)
			.now(clearEffects())
			.hold(70, "")
			.hold(15, "w")
			.now(setEffect("minecraft:levitation", 0, 30))
			.hold(40, "wrj")
			.hold(60, "")
			.hold(10, "");
	}

	static Scenario effectSlowness4() {
		return Scenario.of("effect_slowness_4", "Slowness IV (speed x0.4) walking, sprinting, jumping and sneaking, then slowness VII (speed at zero).")
			.effect("minecraft:slowness", 3)
			.start(0.5, Y0, -20.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(30, "w")
			.hold(30, "wr")
			.hold(20, "wrj")
			.hold(20, "j")
			.hold(20, "wc")
			.hold(15, "")
			.now(setEffect("minecraft:slowness", 6, 100000))
			.hold(5, "")
			.hold(20, "w")
			.hold(20, "wr")
			.hold(15, "wj")
			.hold(15, "j")
			.hold(20, "");
	}

	static Scenario effectSpeedIce() {
		return Scenario.of("effect_speed_ice", "Speed II then speed III on ice, packed ice and blue ice: walking, sprinting, sprint-jumping and sliding to a stop.")
			.effect("minecraft:speed", 1)
			.fill(-6, -64, -20, 6, -64, -9, "minecraft:ice")
			.fill(-6, -64, -8, 6, -64, 3, "minecraft:packed_ice")
			.fill(-6, -64, 4, 6, -64, 20, "minecraft:blue_ice")
			.start(0.5, Y0, -19.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(25, "w")
			.hold(25, "wr")
			.hold(15, "wrj")
			.hold(40, "")
			.look(180.0F, 0.0F)
			.hold(15, "wa")
			.hold(40, "")
			.now(setEffect("minecraft:speed", 2, 100000))
			.hold(5, "")
			.hold(25, "wr")
			.hold(15, "wrj")
			.hold(40, "")
			.look(0.0F, 0.0F)
			.hold(20, "wd", -3.0F, 0.0F)
			.hold(40, "");
	}

	static Scenario effectJumpHoney() {
		return Scenario.of("effect_jump_honey", "Jump boost II and V on honey (jump factor 0.5): jumping in place, sprint-jumping, and jumping from the honey onto a stone step.")
			.effect("minecraft:jump_boost", 1)
			.fill(-6, -64, -6, 6, -64, 6, "minecraft:honey_block")
			.fill(-2, Y0, 8, 2, Y0, 8, "minecraft:stone")
			.start(0.5, Y0, -5.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "j")
			.hold(30, "wrj")
			.hold(15, "")
			.now(setEffect("minecraft:jump_boost", 4, 100000))
			.hold(5, "")
			.hold(40, "j")
			.hold(30, "wj")
			.hold(15, "")
			.hold(40, "wj")
			.hold(20, "");
	}

	static Scenario effectSlowfallSprintJump() {
		Scenario s = Scenario.of("effect_slowfall_sprintjump", "Slow falling with a sprint-jump off a 6-block ledge (a long glide), a walking jump off it, and a jump-in-place on the ground.")
			.effect("minecraft:slow_falling", 0)
			.fill(-1, Y0, -4, 1, Y0 + 5, -2, "minecraft:stone")
			.start(0.5, Y0 + 6, -3.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(14, "wr")
			.hold(25, "wrj")
			.hold(70, "")
			.hold(20, "wr", 3.0F, 0.0F);
		place(s, 0.5, Y0 + 6, -3.5, 0.0F).hold(14, "w").hold(20, "wj").hold(60, "").hold(20, "j").hold(20, "");
		return s;
	}

	// ---------------------------------------------------------------- fluids in unusual shapes

	static final String WATER = "minecraft:water[level=0]";

	// A stone-ringed basin: x, z in [-hx, hx] x [z0, z1] filled with `fluid` up to y = Y0 + depth - 1.
	static Scenario basin(Scenario s, int hx, int z0, int z1, int depth, String fluid) {
		s.fill(-hx - 1, Y0, z0 - 1, hx + 1, Y0 + depth - 1, z1 + 1, "minecraft:stone");
		s.fill(-hx, Y0, z0, hx, Y0 + depth - 1, z1, fluid);
		return s;
	}

	static Scenario waterWaterfall() {
		Scenario s = Scenario.of("water_waterfall", "A 5-high waterfall (falling water) into a 3-deep pool: swimming into the falling column, rising through it against the current, and falling back down with it.");
		basin(s, 3, -3, 3, 3, WATER);
		s.block(-1, Y0 + 8, 0, "minecraft:stone");
		s.block(1, Y0 + 8, 0, "minecraft:stone");
		s.block(0, Y0 + 8, -1, "minecraft:stone");
		s.block(0, Y0 + 8, 1, "minecraft:stone");
		s.block(0, Y0 + 8, 0, WATER);
		s.start(-1.5, Y0 + 1, -1.5, -45.0F, 0.0F)
			.settle(120)
			.hold(10, "")
			.hold(21, "wr")
			.hold(60, "j")
			.hold(30, "")
			.hold(40, "c")
			.hold(20, "w")
			.hold(40, "wj");
		return s;
	}

	static Scenario waterStairsFlow() {
		Scenario s = Scenario.of("water_stairs_flow", "Water from a source on top of a 3-step staircase running down it (flow over the steps and falling water at each step), walked and sprinted with and against the current.");
		s.fill(-2, Y0, -1, 2, Y0 + 4, -1, "minecraft:stone");
		s.fill(-2, Y0, 0, -2, Y0 + 3, 11, "minecraft:stone");
		s.fill(2, Y0, 0, 2, Y0 + 3, 11, "minecraft:stone");
		s.fill(-2, Y0, 12, 2, Y0 + 1, 12, "minecraft:stone");
		s.fill(-1, Y0, 0, 1, Y0 + 2, 2, "minecraft:stone");
		s.fill(-1, Y0, 3, 1, Y0 + 1, 5, "minecraft:stone");
		s.fill(-1, Y0, 6, 1, Y0, 8, "minecraft:stone");
		s.block(0, Y0 + 3, 0, WATER);
		s.start(0.5, Y0, 10.5, 0.0F, 0.0F)
			.settle(160)
			.hold(5, "");
		place(s, 0.5, Y0 + 3, 0.5, 0.0F)
			.hold(15, "")
			.hold(30, "w")
			.hold(30, "wr")
			.hold(30, "")
			.hold(25, "wj")
			.hold(30, "s")
			.hold(30, "wr")
			.hold(20, "a")
			.hold(20, "");
		place(s, 0.5, Y0 + 3, 0.5, 0.0F).hold(40, "")
			.look(180.0F, 0.0F).hold(30, "wr")
			.hold(40, "w");
		return s;
	}

	static Scenario waterloggedSlabsStairs() {
		Scenario s = Scenario.of("waterlogged_slabs_stairs", "A pool floored with waterlogged bottom and top slabs and stairs of every facing and shape (fluid inside non-full blocks): swimming over them, sinking onto them and walking along them.");
		basin(s, 6, -6, 6, 3, WATER);
		String[] facings = {"north", "east", "south", "west"};
		String[] shapes = {"straight", "inner_left", "outer_right", "inner_right", "outer_left"};
		for (int x = -5; x <= 5; x++) {
			s.block(x, Y0, -5, "minecraft:stone_slab[type=bottom,waterlogged=true]");
			s.block(x, Y0, -3, "minecraft:stone_slab[type=top,waterlogged=true]");
			s.block(x, Y0, -1, "minecraft:oak_stairs[facing=" + facings[(x + 5) % 4] + ",half=bottom,shape=" + shapes[(x + 5) % 5] + ",waterlogged=true]");
			s.block(x, Y0, 1, "minecraft:oak_stairs[facing=" + facings[(x + 6) % 4] + ",half=top,shape=" + shapes[(x + 7) % 5] + ",waterlogged=true]");
			s.block(x, Y0, 3, "minecraft:stone_slab[type=bottom,waterlogged=true]");
			s.block(x, Y0 + 1, 3, "minecraft:stone_slab[type=bottom,waterlogged=true]");
			s.block(x, Y0, 5, "minecraft:oak_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=true]");
		}
		s.start(0.5, Y0 + 2, -5.9, 0.0F, 0.0F)
			.settle(60)
			.hold(10, "");
		for (int i = 0; i < 7; i++) {
			s.hold(12, "wj", 0.0F, 0.0F).hold(10, "w", (i % 2 == 0 ? 1.5F : -1.5F), 0.0F).hold(8, "wc");
		}
		s.hold(25, "wr").hold(20, "c").hold(20, "");
		return s;
	}

	static Scenario waterloggedMisc() {
		Scenario s = Scenario.of("waterlogged_misc", "A 3-deep pool of waterlogged fences, panes, iron bars, ladders, chains, lanterns, sea pickles and scaffolding: moving through and around thin and offset shapes with water inside them.");
		basin(s, 6, -6, 6, 3, WATER);
		for (int x = -5; x <= 5; x += 2) {
			s.block(x, Y0, -5, "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=true,west=false]");
			s.block(x, Y0 + 1, -5, "minecraft:oak_fence[east=false,north=false,south=false,waterlogged=true,west=false]");
			s.block(x, Y0, -3, "minecraft:glass_pane[east=false,north=false,south=false,waterlogged=true,west=false]");
			s.block(x, Y0, -1, "minecraft:iron_bars[east=false,north=false,south=false,waterlogged=true,west=false]");
		}
		for (int x : new int[] {-4, -3, -2, 2, 3, 4}) {
			s.block(x, Y0, -4, "minecraft:oak_fence[east=" + (x != -2 && x != 4) + ",north=false,south=false,waterlogged=true,west=" + (x != -4 && x != 2) + "]");
		}
		String[] facings = {"north", "east", "south", "west"};
		for (int x = -5; x <= 5; x += 2) {
			s.block(x, Y0, 1, "minecraft:ladder[facing=" + facings[((x + 5) / 2) % 4] + ",waterlogged=true]");
			s.block(x, Y0 + 1, 1, "minecraft:ladder[facing=" + facings[((x + 5) / 2) % 4] + ",waterlogged=true]");
			s.block(x, Y0, 3, "minecraft:iron_chain[axis=y,waterlogged=true]");
			s.block(x, Y0 + 1, 3, "minecraft:lantern[hanging=true,waterlogged=true]");
			s.block(x, Y0, 5, "minecraft:sea_pickle[pickles=" + (1 + ((x + 5) / 2) % 4) + ",waterlogged=true]");
		}
		s.block(-4, Y0, 3, "minecraft:scaffolding[bottom=false,distance=0,waterlogged=true]");
		s.block(0, Y0, 2, "minecraft:scaffolding[bottom=false,distance=0,waterlogged=true]");
		s.block(4, Y0, 2, "minecraft:scaffolding[bottom=false,distance=0,waterlogged=true]");
		s.start(0.5, Y0 + 2, -5.9, 0.0F, 0.0F)
			.settle(60)
			.hold(10, "");
		for (int i = 0; i < 9; i++) {
			s.hold(14, i % 3 == 0 ? "wa" : (i % 3 == 1 ? "wd" : "w"), (i % 2 == 0 ? 2.0F : -2.0F), 0.0F)
				.hold(10, i % 2 == 0 ? "wj" : "wc");
		}
		s.hold(20, "wr");
		return s;
	}

	static Scenario waterPlants() {
		Scenario s = Scenario.of("water_plants", "A 4-deep pool with kelp columns, seagrass and tall seagrass (water-filled plants with empty collision): swimming, sprint-swimming and sinking through them.");
		basin(s, 4, -4, 4, 4, WATER);
		int[][] kelp = {{-3, -3}, {-1, 2}, {3, 0}, {2, -3}, {-3, 3}};
		for (int[] k : kelp) {
			s.fill(k[0], Y0, k[1], k[0], Y0 + 1, k[1], "minecraft:kelp_plant");
			s.block(k[0], Y0 + 2, k[1], "minecraft:kelp[age=17]");
		}
		s.block(1, Y0, -3, "minecraft:seagrass");
		s.block(-3, Y0, 0, "minecraft:seagrass");
		s.block(0, Y0, 0, "minecraft:tall_seagrass[half=lower]");
		s.block(0, Y0 + 1, 0, "minecraft:tall_seagrass[half=upper]");
		s.block(3, Y0, 3, "minecraft:tall_seagrass[half=lower]");
		s.block(3, Y0 + 1, 3, "minecraft:tall_seagrass[half=upper]");
		s.block(-1, Y0, -1, "minecraft:seagrass");
		s.start(0.5, Y0 + 3, -3.5, 0.0F, 0.0F)
			.settle(60)
			.hold(10, "")
			.hold(30, "w")
			.hold(20, "wc")
			.hold(30, "wr")
			.hold(30, "wr", 3.0F, 0.0F)
			.hold(20, "c")
			.hold(30, "wc", -3.0F, 0.0F)
			.hold(30, "wj")
			.hold(20, "");
		return s;
	}

	static Scenario waterExitSprint() {
		Scenario s = Scenario.of("water_exit_sprint", "Swimming out of a 2-deep pool: onto a flowing beach lower than the water, a bank flush with the surface, a bank with a slab on top (0.6 above the surface) and a bank too high to leave by; sprint-swimming, pressing jump to rise at the shore.");
		s.fill(-9, Y0, -11, -9, Y0 + 1, 9, "minecraft:stone");
		s.fill(9, Y0, -11, 9, Y0 + 1, 9, "minecraft:stone");
		s.fill(-9, Y0, -11, 9, Y0 + 1, -11, "minecraft:stone");
		s.fill(-8, Y0, -10, 8, Y0 + 1, -1, WATER);
		s.fill(-8, Y0, 0, 8, Y0, 9, "minecraft:stone");
		s.fill(-4, Y0 + 1, 0, 8, Y0 + 1, 9, "minecraft:stone");
		s.fill(-2, Y0 + 2, 0, 2, Y0 + 2, 9, SLAB_BOTTOM);
		s.fill(3, Y0 + 2, 0, 8, Y0 + 2, 9, "minecraft:stone");
		s.start(-6.5, Y0 + 1, -9.5, 0.0F, 0.0F)
			.settle(100)
			.hold(10, "")
			.hold(45, "wr")
			.hold(25, "wrj")
			.hold(15, "wr")
			.hold(8, "");
		place(s, -2.5, Y0 + 1, -9.5, 0.0F).hold(45, "wr").hold(25, "wrj").hold(15, "wr").hold(8, "");
		place(s, 0.5, Y0 + 1, -9.5, 0.0F).hold(45, "wr").hold(25, "wrj").hold(15, "wj").hold(8, "");
		place(s, 5.5, Y0 + 1, -9.5, 0.0F).hold(45, "wr").hold(25, "wrj").hold(15, "wj").hold(8, "");
		return s;
	}

	static Scenario waterTunnel() {
		Scenario s = Scenario.of("water_tunnel", "Sprint-swimming down into a 1-high underwater tunnel (the swimming pose fits where the standing pose cannot), through it and out of the other end, surfacing.");
		basin(s, 3, -10, 14, 4, WATER);
		s.fill(-2, Y0 + 1, 2, 2, Y0 + 1, 8, "minecraft:stone");
		s.fill(-2, Y0, 2, -2, Y0, 8, "minecraft:stone");
		s.fill(2, Y0, 2, 2, Y0, 8, "minecraft:stone");
		s.start(0.5, Y0 + 3, -9.0, 0.0F, 40.0F)
			.settle(60)
			.hold(10, "")
			.hold(15, "wr")
			.hold(25, "wr", 0.0F, -1.6F)
			.hold(40, "wr")
			.hold(25, "wr")
			.hold(30, "wr", 0.0F, -3.0F)
			.hold(30, "w")
			.hold(20, "");
		return s;
	}

	static Scenario dolphinsGrace() {
		Scenario s = Scenario.of("dolphins_grace", "Dolphin's grace in a 3-deep channel: swimming, sprint-swimming, diving and surfacing, strafing and sneaking down.")
			.effect("minecraft:dolphins_grace", 0);
		basin(s, 3, -12, 12, 3, WATER);
		s.start(0.5, Y0 + 2, -11.5, 0.0F, 0.0F)
			.settle(60)
			.hold(10, "")
			.hold(30, "w")
			.hold(30, "wr")
			.hold(20, "wr", 0.0F, 2.0F)
			.hold(20, "wr", 0.0F, -3.0F)
			.hold(20, "j")
			.hold(20, "a")
			.hold(20, "c")
			.hold(30, "wr", 2.0F, 0.0F)
			.hold(20, "");
		return s;
	}

	// ---------------------------------------------------------------- lava

	static Scenario lavaUnresisted() {
		Scenario s = Scenario.of("lava_unresisted", "Without fire resistance: stepping into a 1-deep lava pool in a platform (4 damage per hit, ignition), climbing out, and putting the fire out in a water pool.");
		s.fill(-5, Y0, -6, 5, Y0 + 1, 12, "minecraft:stone");
		s.fill(-1, Y0 + 1, 0, 1, Y0 + 1, 0, "minecraft:lava[level=0]");
		s.fill(-1, Y0 + 1, 4, 1, Y0 + 1, 5, WATER);
		s.health(20.0F)
			.start(0.5, Y0 + 2, -3.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(30, "w")
			.hold(12, "wj")
			.hold(25, "w")
			.hold(30, "w")
			.hold(40, "")
			.hold(40, "w");
		return s;
	}

	static Scenario lavaFlow() {
		Scenario s = Scenario.of("lava_flow", "Lava flowing down a pillar and across the floor (slow lava flow with falling lava): walking into the shallow fringe of the flow without fire resistance (ignition, 4 damage per hit), backing out and sprinting into a water pool.");
		s.fill(0, Y0, 0, 0, Y0 + 1, 0, "minecraft:stone");
		s.block(0, Y0 + 2, 0, "minecraft:lava[level=0]");
		s.fill(-13, Y0, -2, -9, Y0, 2, "minecraft:stone");
		s.fill(-12, Y0, -1, -10, Y0, 1, WATER);
		s.health(20.0F)
			.start(-7.5, Y0, 0.5, -90.0F, 0.0F)
			.settle(260)
			.hold(5, "")
			.hold(16, "w")
			.hold(4, "w")
			.look(90.0F, 0.0F)
			.hold(10, "w")
			.hold(25, "wr")
			.hold(20, "wrj")
			.hold(30, "")
			.hold(20, "w");
		return s;
	}

	// ---------------------------------------------------------------- collision shapes

	// Teleport the player `dist` blocks before the point (tx, tz) on a heading of `yaw` degrees, shifted
	// `lateral` blocks to the side, standing on the floor; the first call of a scenario uses the start
	// position instead of a teleport. The caller then holds the keys that walk it into the shape.
	static Scenario probe(Scenario s, double tx, double tz, float yaw, double lateral, double dist) {
		double r = Math.toRadians(yaw);
		double x = tx + dist * Math.sin(r) + lateral * Math.cos(r);
		double z = tz - dist * Math.cos(r) + lateral * Math.sin(r);
		if (s.ticks() == 0) {
			return s.start(x, Y0, z, yaw, 0.0F).hold(5, "");
		}
		return place(s, x, Y0, z, yaw);
	}

	static String stairs(String facing, String half, String shape) {
		return "minecraft:oak_stairs[facing=" + facing + ",half=" + half + ",shape=" + shape + ",waterlogged=false]";
	}

	// A row of stair lanes, one stair per lane, walked into from different headings.
	static Scenario stairsLanes(String name, String description, int z, Object[][] lanes) {
		Scenario s = Scenario.of(name, description);
		for (int i = 0; i < lanes.length; i++) {
			int x = -14 + 4 * i;
			Object[] l = lanes[i];
			s.block(x, Y0, z, stairs((String) l[0], (String) l[1], (String) l[2]));
			probe(s, x + 0.5, z + 0.5, (Float) l[3], (Double) l[4], 2.2).hold((Integer) l[6], (String) l[5]);
		}
		return s;
	}

	static Scenario shapesStairsUp() {
		return stairsLanes("shapes_stairs_up", "Walking, sprinting and jumping into stairs from their low side and diagonally: straight, inner and outer corners, bottom and top halves, every facing.", 0, new Object[][] {
			{"east", "bottom", "inner_left", -90.0F, 0.0, "wr", 25},
			{"north", "bottom", "outer_right", 45.0F, 0.2, "w", 30},
			{"south", "top", "straight", 0.0F, 0.0, "wj", 30},
			{"west", "bottom", "inner_right", 90.0F, -0.2, "w", 28},
			{"north", "top", "outer_left", 180.0F, 0.0, "wrj", 28},
			{"east", "top", "inner_right", -135.0F, 0.3, "w", 30},
			{"south", "bottom", "outer_left", 20.0F, 0.25, "wr", 28},
			{"west", "top", "outer_right", 120.0F, 0.0, "wj", 30}});
	}

	static Scenario shapesStairsDown() {
		return stairsLanes("shapes_stairs_down", "Walking, sprinting and jumping into stairs from the high side, the sides and at corners: the same shapes approached against their step.", 8, new Object[][] {
			{"east", "bottom", "straight", 90.0F, 0.0, "w", 25},
			{"north", "bottom", "inner_left", 0.0F, 0.0, "wr", 28},
			{"south", "bottom", "outer_right", 180.0F, 0.1, "w", 28},
			{"west", "bottom", "inner_left", -90.0F, 0.0, "wj", 28},
			{"east", "top", "outer_left", 90.0F, -0.15, "w", 28},
			{"north", "top", "straight", 0.0F, 0.0, "w", 25},
			{"south", "top", "inner_right", 180.0F, 0.0, "wj", 28},
			{"west", "top", "inner_left", -90.0F, 0.2, "wr", 28}});
	}

	static Scenario shapesSlabCorners() {
		Scenario s = Scenario.of("shapes_slab_corners", "Slabs and stone met at corners: a 2x2 slab block taken diagonally, a stone block and a slab touching only at a corner, a top slab, a checkerboard of slabs crossed diagonally, snow layers 5 (steps up) and 6 (0.625, too high) and an ascending snow ramp.");
		for (int x = -14; x <= -13; x++) {
			for (int z = 0; z <= 1; z++) {
				s.block(x, Y0, z, SLAB_BOTTOM);
			}
		}
		probe(s, -13.0, 1.0, 45.0F, 0.0, 2.5).hold(30, "w");
		s.block(-8, Y0, 0, "minecraft:stone");
		s.block(-7, Y0, 1, SLAB_BOTTOM);
		probe(s, -7.0, 1.0, -45.0F, 0.0, 2.5).hold(10, "w").hold(25, "wj");
		s.block(-2, Y0, 0, "minecraft:stone_slab[type=top,waterlogged=false]");
		probe(s, -1.5, 0.5, 0.0F, 0.0, 2.2).hold(25, "w");
		for (int x = 3; x <= 8; x++) {
			for (int z = -3; z <= 2; z++) {
				if ((x + z) % 2 == 0) {
					s.block(x, Y0, z, SLAB_BOTTOM);
				}
			}
		}
		probe(s, 6.0, -0.5, 45.0F, 0.0, 6.0).hold(60, "wr");
		s.block(12, Y0, 0, "minecraft:snow[layers=5]");
		s.block(14, Y0, 0, "minecraft:snow[layers=6]");
		probe(s, 12.5, 0.5, 0.0F, 0.0, 2.2).hold(25, "w");
		probe(s, 14.5, 0.5, 0.0F, 0.0, 2.2).hold(25, "w").hold(12, "wj");
		for (int layers = 2; layers <= 7; layers++) {
			s.block(14 + layers, Y0, 6, "minecraft:snow[layers=" + layers + "]");
		}
		s.block(22, Y0, 6, "minecraft:stone");
		probe(s, 17.5, 6.5, -90.0F, 0.0, 2.2).hold(30, "wr");
		return s;
	}

	// ---------------------------------------------------------------- containers and small blocks

	static Scenario shapesContainers() {
		Scenario s = Scenario.of("shapes_containers", "Chest and ender chest (0.875 high), enchanting table (0.75), a cauldron (standing inside it), composters of three fill levels and a hopper: walked into, jumped onto and off.");
		s.block(-16, Y0, 0, "minecraft:chest[facing=north,type=single,waterlogged=false]");
		probe(s, -15.5, 0.5, 0.0F, 0.0, 2.2).hold(15, "w").hold(15, "wj").hold(10, "w");
		s.block(-12, Y0, 0, "minecraft:ender_chest[facing=east,waterlogged=false]");
		probe(s, -11.5, 0.5, 45.0F, 0.0, 2.2).hold(15, "w").hold(15, "wrj").hold(10, "w");
		s.block(-8, Y0, 0, "minecraft:enchanting_table");
		probe(s, -7.5, 0.5, -90.0F, 0.0, 2.2).hold(15, "wr").hold(15, "wj").hold(10, "w");
		s.block(-4, Y0, 0, "minecraft:cauldron");
		place(s, -3.5, Y0 + 2.0, 0.5, 0.0F).hold(15, "").hold(15, "w").hold(12, "a").hold(10, "d").hold(12, "wj").hold(10, "w");
		for (int level = 0; level <= 8; level += 4) {
			int x = level;
			s.block(x, Y0, 0, "minecraft:composter[level=" + level + "]");
			probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.2).hold(15, "w").hold(15, "wj").hold(10, "w");
		}
		s.block(12, Y0, 0, "minecraft:hopper[enabled=true,facing=down]");
		probe(s, 12.5, 0.5, 90.0F, 0.0, 2.2).hold(15, "w").hold(15, "wj").hold(10, "w");
		return s;
	}

	static Scenario shapesSmallA() {
		Scenario s = Scenario.of("shapes_small_a", "Cakes with 0, 3 and 6 bites, a flower pot, a daylight sensor, anvils and lecterns: small and low shapes walked over, into and jumped onto.");
		int x = -18;
		for (int bites : new int[] {0, 3, 6}) {
			s.block(x, Y0, 0, "minecraft:cake[bites=" + bites + "]");
			probe(s, x + 0.5, 0.5, 0.0F, bites == 3 ? 0.35 : 0.0, 2.2).hold(15, "w").hold(15, "wj").hold(10, "w");
			x += 3;
		}
		s.block(x, Y0, 0, "minecraft:flower_pot");
		probe(s, x + 0.5, 0.5, 45.0F, 0.0, 2.2).hold(25, "w").hold(12, "wj");
		x += 3;
		s.block(x, Y0, 0, "minecraft:daylight_detector[inverted=false,power=0]");
		probe(s, x + 0.5, 0.5, -90.0F, 0.0, 2.2).hold(25, "wr");
		x += 3;
		for (String facing : new String[] {"north", "east"}) {
			s.block(x, Y0, 0, "minecraft:anvil[facing=" + facing + "]");
			probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.2).hold(15, "w").hold(15, "wj").hold(10, "w");
			x += 3;
		}
		for (String book : new String[] {"false", "true"}) {
			s.block(x, Y0, 0, "minecraft:lectern[facing=north,has_book=" + book + ",powered=false]");
			probe(s, x + 0.5, 0.5, 90.0F, 0.0, 2.2).hold(15, "w").hold(15, "wj").hold(10, "w");
			x += 3;
		}
		return s;
	}

	static Scenario shapesSmallB() {
		Scenario s = Scenario.of("shapes_small_b", "Lanterns, chains, end rods, a stonecutter, campfire, grindstone, bell, brewing stand, candles and an end portal frame: thin, offset and odd shapes walked into and over.");
		int x = -18;
		String[] blocks = {
			"minecraft:lantern[hanging=false,waterlogged=false]",
			"minecraft:lantern[hanging=true,waterlogged=false]",
			"minecraft:iron_chain[axis=y,waterlogged=false]",
			"minecraft:iron_chain[axis=x,waterlogged=false]",
			"minecraft:end_rod[facing=up]",
			"minecraft:end_rod[facing=north]",
			"minecraft:stonecutter[facing=north]",
			"minecraft:campfire[facing=north,lit=false,signal_fire=false,waterlogged=false]",
			"minecraft:grindstone[face=floor,facing=north]",
			"minecraft:bell[attachment=floor,facing=north,powered=false]",
			"minecraft:brewing_stand[has_bottle_0=false,has_bottle_1=false,has_bottle_2=false]",
			"minecraft:candle[candles=3,lit=false,waterlogged=false]",
			"minecraft:end_portal_frame[eye=false,facing=north]"};
		float[] yaws = {0.0F, 45.0F, -45.0F, 90.0F, 0.0F, 135.0F, -90.0F, 20.0F, 0.0F, -20.0F, 60.0F, 0.0F, 180.0F};
		for (int i = 0; i < blocks.length; i++) {
			s.block(x, Y0, 0, blocks[i]);
			probe(s, x + 0.5, 0.5, yaws[i], (i % 3 - 1) * 0.15, 2.0).hold(12, "w").hold(10, "wj");
			x += 3;
		}
		return s;
	}

	static Scenario shapesFencesWalls() {
		Scenario s = Scenario.of("shapes_fences_walls", "Fence gates (closed 1.5 high, open, in-wall), walls (tall sides, corners, low), glass pane corners, iron bars, open and closed doors and top-half trapdoors: walked into straight and diagonally, jumped at.");
		int x = -20;
		s.block(x, Y0, 0, "minecraft:oak_fence_gate[facing=north,in_wall=false,open=false,powered=false]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(15, "w").hold(15, "wj");
		x += 3;
		s.block(x, Y0, 0, "minecraft:oak_fence_gate[facing=east,in_wall=true,open=false,powered=false]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(15, "w").hold(15, "wj");
		x += 3;
		s.block(x, Y0, 0, "minecraft:oak_fence_gate[facing=north,in_wall=false,open=true,powered=false]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(25, "w");
		x += 3;
		s.block(x, Y0, 0, "minecraft:cobblestone_wall[east=tall,north=none,south=low,up=true,waterlogged=false,west=none]");
		s.block(x + 1, Y0, 0, "minecraft:cobblestone_wall[east=none,north=none,south=none,up=true,waterlogged=false,west=tall]");
		probe(s, x + 0.5, 0.5, 45.0F, 0.0, 2.0).hold(15, "w").hold(15, "wj");
		x += 4;
		s.block(x, Y0, 0, "minecraft:cobblestone_wall[east=low,north=low,south=none,up=true,waterlogged=false,west=none]");
		probe(s, x + 0.5, 0.5, -45.0F, 0.0, 2.0).hold(15, "w").hold(15, "wj");
		x += 3;
		s.block(x, Y0, 0, "minecraft:glass_pane[east=true,north=true,south=false,waterlogged=false,west=false]");
		probe(s, x + 0.5, 0.5, 45.0F, 0.0, 2.0).hold(25, "w");
		x += 3;
		s.block(x, Y0, 0, "minecraft:iron_bars[east=true,north=true,south=true,waterlogged=false,west=true]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.2, 2.0).hold(25, "wr");
		x += 3;
		s.block(x, Y0, 0, "minecraft:oak_door[facing=east,half=lower,hinge=left,open=true,powered=false]");
		s.block(x, Y0 + 1, 0, "minecraft:oak_door[facing=east,half=upper,hinge=left,open=true,powered=false]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(25, "w");
		x += 3;
		s.block(x, Y0, 0, "minecraft:oak_door[facing=north,half=lower,hinge=right,open=true,powered=false]");
		s.block(x, Y0 + 1, 0, "minecraft:oak_door[facing=north,half=upper,hinge=right,open=true,powered=false]");
		probe(s, x + 0.5, 0.5, -90.0F, 0.0, 2.0).hold(25, "w");
		x += 3;
		s.block(x, Y0, 0, "minecraft:oak_trapdoor[facing=north,half=top,open=false,powered=false,waterlogged=false]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(20, "w");
		x += 3;
		s.block(x, Y0, 0, "minecraft:oak_trapdoor[facing=south,half=top,open=true,powered=false,waterlogged=false]");
		probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(20, "w").hold(12, "wj");
		return s;
	}

	// Position-offset shapes: bamboo and pointed dripstone collide at a position-dependent shift.
	static Scenario shapesBamboo() {
		Scenario s = Scenario.of("shapes_bamboo", "A walled bamboo grove on a checkerboard: the collision shape of each stalk is offset by a hash of its position, so the player runs, sprints, jumps and strafes through many different offsets.");
		for (int x = -10; x <= 1; x++) {
			for (int z = -5; z <= 6; z++) {
				if ((x + z) % 2 == 0) {
					s.block(x, Y0, z, "minecraft:bamboo[age=0,leaves=none,stage=0]");
				}
			}
		}
		s.fill(-12, Y0, -7, 3, Y0 + 1, -7, "minecraft:stone");
		s.fill(-12, Y0, 8, 3, Y0 + 1, 8, "minecraft:stone");
		s.fill(-12, Y0, -7, -12, Y0 + 1, 8, "minecraft:stone");
		s.fill(3, Y0, -7, 3, Y0 + 1, 8, "minecraft:stone");
		s.start(-10.5, Y0, -6.4, 0.0F, 0.0F)
			.hold(5, "")
			.hold(25, "w")
			.hold(30, "w", 1.5F, 0.0F)
			.hold(25, "wr", -2.0F, 0.0F)
			.hold(20, "wa")
			.hold(30, "wr", 2.5F, 0.0F)
			.hold(15, "wj")
			.hold(30, "wd", -1.0F, 0.0F)
			.hold(20, "s")
			.hold(40, "wr", 3.0F, 0.0F);
		place(s, -9.5, Y0, 5.7, 180.0F).hold(40, "w", -1.0F, 0.0F).hold(30, "wr").hold(25, "wj", 3.0F, 0.0F);
		return s;
	}

	static Scenario shapesDripstone() {
		Scenario s = Scenario.of("shapes_dripstone", "Pointed dripstone of every thickness (offset by a position hash), big dripleaf in four tilt states with its stem and a small dripleaf: walked into and jumped onto.");
		String[] thickness = {"tip", "frustum", "middle", "base", "tip_merge"};
		int x = -20;
		for (int i = 0; i < thickness.length; i++) {
			for (String dir : new String[] {"up", "down"}) {
				s.block(x, Y0, 0, "minecraft:pointed_dripstone[thickness=" + thickness[i] + ",vertical_direction=" + dir + ",waterlogged=false]");
				probe(s, x + 0.5, 0.5, (i * 40.0F) - 80.0F, 0.1 * (i - 2), 2.0).hold(14, "w").hold(12, "wj");
				x += 3;
			}
		}
		for (String tilt : new String[] {"none", "unstable", "partial", "full"}) {
			s.block(x, Y0, 0, "minecraft:big_dripleaf[facing=north,tilt=" + tilt + ",waterlogged=false]");
			probe(s, x + 0.5, 0.5, 0.0F, 0.0, 2.0).hold(12, "w");
			x += 3;
		}
		return s;
	}

	// ---------------------------------------------------------------- projectiles

	// Like `spawn`, but the projectile's owner is the player (so an ender pearl teleports it).
	static Scenario.ServerAction spawnOwned(String type, double x, double y, double z, double dx, double dy, double dz) {
		return (level, player, log) -> {
			EntityType<?> t = EntityType.byString(type).orElseThrow(() -> new IllegalArgumentException(type));
			Entity e = t.create(level, EntitySpawnReason.COMMAND);
			if (e == null) {
				throw new IllegalStateException("could not create " + type);
			}
			e.snapTo(x, y, z, 0.0F, 0.0F);
			e.setDeltaMovement(dx, dy, dz);
			if (e instanceof net.minecraft.world.entity.projectile.Projectile proj) {
				proj.setOwner(player);
			}
			level.addFreshEntity(e);
			log.addProperty("op", "spawn");
			log.addProperty("type", type);
			log.addProperty("id", e.getId());
			log.add("entity", Snapshot.entity(e));
		};
	}

	// What non-player entities exist in the arena (to spot a chick hatched by an egg).
	static Scenario.ServerAction census() {
		return (level, player, log) -> {
			log.addProperty("op", "census");
			JsonObject counts = new JsonObject();
			for (Entity e : OracleArena.entitiesInArena(level)) {
				String k = EntityType.getKey(e.getType()).toString();
				counts.addProperty(k, counts.has(k) ? counts.get(k).getAsInt() + 1 : 1);
			}
			log.add("entities", counts);
		};
	}

	// A shot aimed from `from` at the point `to`, flying with the given speed (blocks per tick).
	static Scenario.ServerAction shot(String type, double[] from, double[] to, double speed) {
		double dx = to[0] - from[0];
		double dy = to[1] - from[1];
		double dz = to[2] - from[2];
		double len = Math.sqrt(dx * dx + dy * dy + dz * dz);
		return spawn(type, from[0], from[1], from[2], dx / len * speed, dy / len * speed, dz / len * speed);
	}

	static Scenario projArrowWalking() {
		Scenario s = Scenario.of("proj_arrow_walking", "Arrows hitting a walking, sprinting and jumping player head-on, from behind and from the side: damage, knockback in each situation.")
			.trackProjectiles();
		// walking, arrow head-on from 11 blocks ahead
		s.start(0.5, Y0, -12.5, 0.0F, 0.0F).hold(10, "").hold(6, "w")
			.now(spawn("minecraft:arrow", 0.5, Y0 + 1.2, -1.0, 0.0, 0.03, -2.0))
			.hold(30, "w")
			.hold(15, "");
		// walking, arrow from behind (faster than the player)
		s.now(heal());
		place(s, 0.5, Y0, -10.5, 0.0F).hold(8, "w")
			.now(spawn("minecraft:arrow", 0.5, Y0 + 1.2, -17.0, 0.0, 0.03, 2.5))
			.hold(30, "w")
			.hold(15, "");
		// sprinting east, arrow crossing from the south (the player's right)
		s.now(heal());
		place(s, -10.5, Y0, 0.5, -90.0F).hold(8, "wr")
			.now(spawn("minecraft:arrow", -8.0, Y0 + 1.2, 6.5, 0.0, 0.03, -2.0))
			.hold(30, "wr")
			.hold(15, "");
		// sprint-jumping north, two arrows crossing from the west one tick apart
		s.now(heal());
		place(s, 0.5, Y0, 12.5, 180.0F).hold(6, "wr").hold(8, "wrj")
			.now(spawn("minecraft:arrow", -9.0, Y0 + 1.6, 7.2, 2.5, 0.0, 0.0))
			.hold(1, "wrj")
			.now(spawn("minecraft:arrow", -9.0, Y0 + 1.6, 6.9, 2.5, 0.0, 0.0))
			.hold(30, "wrj")
			.hold(15, "");
		// walking backwards, arrow coming in at a diagonal
		s.now(heal());
		place(s, 8.5, Y0, 8.5, 90.0F).hold(8, "s")
			.now(spawn("minecraft:arrow", 8.0, Y0 + 1.0, 18.0, 0.6, 0.02, -2.0))
			.hold(30, "s")
			.now(census())
			.hold(15, "");
		return s;
	}

	static Scenario projArrowSteep() {
		Scenario s = Scenario.of("proj_arrow_steep", "Arrows dropping on the player from steep angles (80, 60 and 45 degrees from above) while it stands, walks, sprints and jumps.")
			.trackProjectiles();
		double[] c = {0.5, Y0 + 0.9, 0.5};
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F).hold(10, "")
			.now(shot("minecraft:arrow", new double[] {0.5, Y0 + 11.0, 0.1}, c, 2.8))
			.hold(30, "");
		s.now(heal());
		place(s, 0.5, Y0, 0.5, 0.0F).hold(10, "")
			.now(shot("minecraft:arrow", new double[] {0.5, Y0 + 9.0, -4.0}, c, 2.8))
			.hold(30, "");
		s.now(heal());
		place(s, 0.5, Y0, 0.5, 0.0F).hold(10, "")
			.now(shot("minecraft:arrow", new double[] {-6.5, Y0 + 7.0, 0.5}, c, 2.5))
			.hold(30, "");
		// walking south: aim ahead of the player
		s.now(heal());
		place(s, 0.5, Y0, -9.5, 0.0F).hold(6, "w")
			.now(shot("minecraft:arrow", new double[] {0.5, Y0 + 12.0, -7.65}, new double[] {0.5, Y0 + 0.9, -7.55}, 2.8))
			.hold(30, "w").hold(10, "");
		// sprinting east: aim ahead
		s.now(heal());
		place(s, -12.5, Y0, 8.5, -90.0F).hold(6, "wr")
			.now(shot("minecraft:arrow", new double[] {-10.4, Y0 + 8.0, 6.0}, new double[] {-10.0, Y0 + 0.9, 8.5}, 2.8))
			.hold(30, "wr").hold(10, "");
		// jumping in place, arrow from 45 degrees
		s.now(heal());
		place(s, 6.5, Y0, 0.5, 0.0F).hold(8, "j")
			.now(shot("minecraft:arrow", new double[] {6.5, Y0 + 7.0, -5.0}, new double[] {6.5, Y0 + 1.5, 0.5}, 2.5))
			.hold(30, "j").hold(10, "")
			.now(census())
			.hold(5, "");
		return s;
	}

	static Scenario projSnowEggMoving() {
		Scenario s = Scenario.of("proj_snow_egg_moving", "Snowballs and eggs thrown at a walking, sprinting and sprint-jumping player (zero-damage hits that still remove the projectile), and some that miss.")
			.trackProjectiles();
		s.start(0.5, Y0, -12.5, 0.0F, 0.0F).hold(10, "").hold(6, "w")
			.now(spawn("minecraft:snowball", 0.5, Y0 + 1.2, -3.0, 0.0, 0.05, -1.5))
			.hold(30, "w").hold(10, "");
		place(s, -10.5, Y0, 0.5, -90.0F).hold(8, "wr")
			.now(spawn("minecraft:snowball", -4.0, Y0 + 1.2, 6.5, 0.0, 0.05, -1.3))
			.hold(30, "wr").hold(10, "");
		place(s, 0.5, Y0, 14.5, 180.0F).hold(6, "wr").hold(8, "wrj")
			.now(spawn("minecraft:snowball", 6.0, Y0 + 1.6, 9.0, -1.4, 0.0, 0.0))
			.hold(30, "wrj").hold(10, "");
		place(s, 6.5, Y0, -10.5, 0.0F).hold(6, "w")
			.now(spawn("minecraft:egg", 6.5, Y0 + 1.2, 1.0, 0.0, 0.05, -1.2))
			.hold(30, "w").hold(10, "")
			.now(census());
		place(s, -6.5, Y0, 10.5, 180.0F).hold(8, "w")
			.now(spawn("minecraft:snowball", -12.0, Y0 + 3.0, 6.0, 1.2, -0.2, 0.0))
			.hold(30, "w").hold(10, "")
			.now(census())
			.hold(5, "");
		return s;
	}

	static Scenario projArrowWater() {
		Scenario s = Scenario.of("proj_arrow_water", "Arrows landing in a pool (water drag, sinking, sticking into the bottom) and hitting the player standing and swimming in it.")
			.trackProjectiles();
		s.fill(-6, Y0, -6, 6, Y0 + 3, 6, "minecraft:stone");
		s.fill(-5, Y0, -5, 5, Y0 + 3, 5, WATER);
		s.start(0.5, Y0 + 2, -4.5, 0.0F, 0.0F).settle(60).hold(10, "")
			.now(spawn("minecraft:arrow", -4.0, Y0 + 7.0, 0.0, 0.3, -0.5, 0.0))
			.hold(30, "")
			.now(spawn("minecraft:arrow", 4.0, Y0 + 5.0, 3.0, -0.5, -0.1, -0.4))
			.hold(30, "")
			.now(spawn("minecraft:arrow", 0.5, Y0 + 2.0, -1.5, 0.0, 0.0, -3.0))
			.hold(10, "w")
			.hold(30, "wr")
			.now(spawn("minecraft:spectral_arrow", -4.0, Y0 + 1.0, 2.0, 1.2, 0.05, 0.0))
			.hold(30, "wr")
			.hold(10, "j")
			.now(census())
			.hold(5, "");
		return s;
	}

	static Scenario projEnderPearl() {
		Scenario s = Scenario.of("proj_ender_pearl", "Ender pearls thrown by the player and landing on the floor, a wall and a slab: the player is teleported to the landing spot and takes 5 damage, standing and moving.")
			.trackProjectiles()
			.fill(-3, Y0, 12, 3, Y0 + 4, 12, "minecraft:stone")
			.fill(8, Y0, 4, 10, Y0, 6, SLAB_BOTTOM);
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F).hold(10, "")
			.now(spawnOwned("minecraft:ender_pearl", 0.5, Y0 + 1.6, 0.5, 0.2, 0.3, 0.45))
			.hold(50, "")
			.hold(15, "w");
		s.now(heal());
		place(s, 0.5, Y0, 0.5, 0.0F)
			.now(spawnOwned("minecraft:ender_pearl", 0.5, Y0 + 1.6, 0.5, 0.0, 0.2, 0.9))
			.hold(40, "w")
			.hold(15, "");
		s.now(heal());
		place(s, 0.5, Y0, 0.5, 0.0F)
			.now(spawnOwned("minecraft:ender_pearl", 0.5, Y0 + 1.6, 0.5, 0.45, 0.25, 0.2))
			.hold(40, "")
			.hold(10, "wr");
		s.now(heal());
		place(s, 0.5, Y0, 0.5, 0.0F)
			.now(spawnOwned("minecraft:ender_pearl", 0.5, Y0 + 1.6, 0.5, -0.35, 0.2, 0.3))
			.hold(40, "wrj")
			.hold(20, "")
			.now(census())
			.hold(5, "");
		return s;
	}

	// ---------------------------------------------------------------- three more seeded random courses

	// Builds a layout that can be mirrored in x and/or z (so three courses share one design but meet
	// every obstacle from a different side); `facing` properties are mirrored with the coordinates.
	static final class Course {
		final Scenario s;
		final int mx;
		final int mz;

		Course(Scenario s, int mx, int mz) {
			this.s = s;
			this.mx = mx;
			this.mz = mz;
		}

		String face(String f) {
			if (mz < 0 && f.equals("north")) {
				return "south";
			}
			if (mz < 0 && f.equals("south")) {
				return "north";
			}
			if (mx < 0 && f.equals("east")) {
				return "west";
			}
			if (mx < 0 && f.equals("west")) {
				return "east";
			}
			return f;
		}

		void fill(int x0, int y0, int z0, int x1, int y1, int z1, String state) {
			s.fill(mx * x0, y0, mz * z0, mx * x1, y1, mz * z1, state);
		}

		void block(int x, int y, int z, String state) {
			s.block(mx * x, y, mz * z, state);
		}

		// A stone-walled basin of water, `depth` blocks deep.
		void pool(int x0, int z0, int x1, int z1, int depth) {
			fill(x0 - 1, Y0, z0 - 1, x1 + 1, Y0 + depth - 1, z1 + 1, "minecraft:stone");
			fill(x0, Y0, z0, x1, Y0 + depth - 1, z1, WATER);
		}

		// A flight of `n` stairs rising towards +z (mirrored with the course) from z0.
		void stairRun(int x0, int x1, int z0, int n) {
			for (int k = 0; k < n; k++) {
				if (k > 0) {
					fill(x0, Y0, z0 + k, x1, Y0 + k - 1, z0 + k, "minecraft:stone");
				}
				fill(x0, Y0 + k, z0 + k, x1, Y0 + k, z0 + k, "minecraft:oak_stairs[facing=" + face("south") + ",half=bottom,shape=straight,waterlogged=false]");
			}
		}

		// A tower with a ladder on its north face, a 3x3 top to step onto.
		void ladderTower(int x, int z, int height) {
			fill(x - 1, Y0, z, x + 1, Y0 + height - 1, z + 2, "minecraft:stone");
			fill(x, Y0, z - 1, x, Y0 + height - 1, z - 1, "minecraft:ladder[facing=" + face("north") + ",waterlogged=false]");
		}

		// A fence line along x with a gate in it.
		void fenceLine(int x0, int x1, int z, int gateX) {
			for (int x = x0; x <= x1; x++) {
				if (x == gateX) {
					block(x, Y0, z, "minecraft:oak_fence_gate[facing=" + face("north") + ",in_wall=false,open=" + (gateX % 2 == 0) + ",powered=false]");
				} else {
					block(x, Y0, z, "minecraft:oak_fence[east=" + ((mx > 0 ? x < x1 : x > x0)) + ",north=false,south=false,waterlogged=false,west=" + ((mx > 0 ? x > x0 : x < x1)) + "]");
				}
			}
		}
	}

	static void courseLayout(Course c) {
		Scenario s = c.s;
		// perimeter wall: a 25 x 25 room around the start
		s.fill(-12, Y0, -12, 12, Y0 + 2, -12, "minecraft:stone");
		s.fill(-12, Y0, 12, 12, Y0 + 2, 12, "minecraft:stone");
		s.fill(-12, Y0, -12, -12, Y0 + 2, 12, "minecraft:stone");
		s.fill(12, Y0, -12, 12, Y0 + 2, 12, "minecraft:stone");
		// a deep pool beside the start, a bubble column up over soul sand, one down over magma
		c.pool(3, -3, 8, 2, 3);
		c.pool(-9, -10, -5, -6, 5);
		c.block(-7, -64, -8, "minecraft:soul_sand");
		c.fill(-7, Y0, -8, -7, Y0 + 4, -8, "minecraft:bubble_column[drag=false]");
		c.pool(-9, 6, -5, 10, 5);
		c.block(-7, -64, 8, "minecraft:magma_block");
		c.fill(-7, Y0, 8, -7, Y0 + 4, 8, "minecraft:bubble_column[drag=true]");
		// slime pad with a pillar, honey floor and a honey wall
		c.fill(3, -64, 5, 10, -64, 10, "minecraft:slime_block");
		c.fill(6, Y0, 7, 6, Y0 + 2, 7, "minecraft:slime_block");
		c.fill(3, -64, -10, 10, -64, -6, "minecraft:honey_block");
		c.fill(-3, Y0, -3, -3, Y0 + 3, 3, "minecraft:honey_block");
		// stairs, two fence lines with gates, a ladder tower, slabs, snow and carpet
		c.stairRun(-2, 2, -10, 4);
		c.fenceLine(-2, 2, 4, 0);
		c.fenceLine(-2, 2, -5, 1);
		c.ladderTower(0, 9, 5);
		c.fill(-2, Y0, -2, 0, Y0, -2, "minecraft:stone_slab[type=bottom,waterlogged=false]");
		c.fill(-2, Y0, 2, 0, Y0, 2, "minecraft:snow[layers=3]");
		c.fill(1, Y0, 6, 2, Y0, 6, "minecraft:white_carpet");
		c.fill(-1, Y0, -8, 1, Y0, -8, "minecraft:cobblestone_wall[east=low,north=none,south=none,up=true,waterlogged=false,west=low]");
	}

	static void randomWalk(Scenario s, long seed, int segments, String[] moves, float turn, int minLen, int maxLen) {
		Random r = new Random(seed);
		float yaw = 0.0F;
		float pitch = 0.0F;
		for (int i = 0; i < segments; i++) {
			String keys = moves[r.nextInt(moves.length)];
			int len = minLen + r.nextInt(maxLen - minLen + 1);
			float yawStep = (r.nextFloat() - 0.5F) * turn;
			float pitchStep = r.nextInt(5) == 0 ? (r.nextFloat() - 0.5F) * 2.0F : 0.0F;
			if (Math.abs(pitch + pitchStep * len) > 55.0F) {
				pitchStep = -pitchStep;
			}
			s.hold(len, keys, yawStep, pitchStep);
			yaw += yawStep * len;
			pitch += pitchStep * len;
			if (Math.abs(pitch) > 70.0F) {
				pitch = 0.0F;
				s.look(yaw, 0.0F);
			}
		}
		s.hold(10, "");
	}

	static Scenario courseC() {
		Scenario s = Scenario.of("random_course_c", "Seeded random inputs (seed 3) across a walled room of pools, slime, honey, bubble columns, stairs, fences, a ladder tower, slabs and snow, restarting by teleport in the deep pool, in both bubble columns and at the ladder.");
		courseLayout(new Course(s, 1, 1));
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F).hold(5, "");
		courseRun(new Course(s, 1, 1), 3L, new String[] {"w", "w", "wr", "wrj", "wj", "wa", "wd", "a", "d", "s", "", "j", "wc", "c", "wrj", "wr"}, 12.0F, 5, 24, 11);
		return s;
	}

	static Scenario courseD() {
		Scenario s = Scenario.of("random_course_d", "Seeded random inputs (seed 4, jump and sprint heavy) across the room mirrored through its centre, restarting by teleport in the deep pool, in both bubble columns and at the ladder.");
		courseLayout(new Course(s, -1, -1));
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F).hold(5, "");
		courseRun(new Course(s, -1, -1), 4L, new String[] {"wrj", "wrj", "wr", "wj", "w", "wr", "j", "wa", "wd", "wrj", "wrj", "", "sj", "wrc"}, 16.0F, 4, 20, 11);
		return s;
	}

	static Scenario courseE() {
		Scenario s = Scenario.of("random_course_e", "Seeded random inputs (seed 5, sneak and strafe heavy) across the room mirrored in x, restarting by teleport in the deep pool, in both bubble columns and at the ladder.");
		courseLayout(new Course(s, -1, 1));
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F).hold(5, "");
		courseRun(new Course(s, -1, 1), 5L, new String[] {"wc", "wc", "w", "wr", "c", "wcj", "wj", "wrj", "wa", "wd", "sd", "sa", "", "ac", "dc"}, 10.0F, 6, 26, 10);
		return s;
	}

	static Scenario fallEdgesB() {
		Scenario s = Scenario.of("fall_edges_b", "More falls around the damage thresholds: 2.9375 (just under 3), 3.4375, 3.5625, exactly 4.0, 4.0625, 3.875, 4.625 and 4.75 blocks onto carpet, a bed, slabs, snow, a daylight sensor, hay and stone.");
		String bed = "minecraft:red_bed[facing=south,occupied=false,part=foot]";
		String sensor = "minecraft:daylight_detector[inverted=false,power=0]";
		Object[][] lanes = {
			// tower height, cap block, cap collision height, landing block, keys, ticks
			{3, null, 0.0, "minecraft:white_carpet", "w", 8},
			{4, null, 0.0, bed, "wr", 6},
			{4, "minecraft:white_carpet", 0.0625, SLAB_BOTTOM, "w", 8},
			{4, null, 0.0, null, "w", 8},
			{4, "minecraft:white_carpet", 0.0625, null, "wr", 6},
			{4, null, 0.0, "minecraft:snow[layers=2]", "w", 8},
			{5, null, 0.0, sensor, "wr", 6},
			{5, "minecraft:snow[layers=7]", 0.75, "minecraft:hay_block[axis=y]", "w", 8}};
		for (int i = 0; i < lanes.length; i++) {
			int x = -17 + 5 * i;
			int h = (Integer) lanes[i][0];
			String cap = (String) lanes[i][1];
			String land = (String) lanes[i][3];
			s.fill(x - 1, Y0, -3, x + 1, Y0 + h - 1, -1, "minecraft:stone");
			if (cap != null) {
				s.fill(x - 1, Y0 + h, -3, x + 1, Y0 + h, -1, cap);
			}
			if (land != null) {
				s.fill(x - 1, Y0, 0, x + 1, Y0, 5, land);
			}
		}
		for (int i = 0; i < lanes.length; i++) {
			int x = -17 + 5 * i;
			double top = Y0 + (Integer) lanes[i][0] + (Double) lanes[i][2];
			if (i == 0) {
				s.start(x + 0.5, top, -0.2, 0.0F, 0.0F).hold(5, "");
			} else {
				place(s, x + 0.5, top, -0.2, 0.0F);
			}
			s.hold((Integer) lanes[i][5], (String) lanes[i][4]).hold(36, "");
		}
		return s;
	}

	// Back to full health (arrows hit hard; one scenario holds several lanes).
	static Scenario.ServerAction heal() {
		return (level, player, log) -> {
			player.setHealth(20.0F);
			log.addProperty("op", "heal");
		};
	}

	// Put the fire out and heal (lava lanes follow one another).
	static Scenario.ServerAction recover() {
		return (level, player, log) -> {
			player.setHealth(20.0F);
			player.clearFire();
			log.addProperty("op", "recover");
		};
	}

	static Scenario lavaLanes() {
		Scenario s = Scenario.of("lava_lanes", "The fringe of a lava flow from four sides without fire resistance (the tip cells are shallow lava, deeper cells trap the player): walking in and backing out, sprinting in, sprint-jumping into it; health and fire are restored between the lanes.");
		s.fill(0, Y0, 0, 0, Y0 + 1, 0, "minecraft:stone");
		s.block(0, Y0 + 2, 0, "minecraft:lava[level=0]");
		s.health(20.0F)
			.start(-7.5, Y0, 0.5, -90.0F, 0.0F)
			.settle(260)
			.hold(5, "")
			.hold(16, "w")
			.hold(4, "w")
			.look(90.0F, 0.0F)
			.hold(10, "w")
			.hold(15, "wr")
			.hold(10, "");
		s.now(recover());
		place(s, 0.5, Y0, -8.0, 0.0F).hold(19, "w").hold(4, "w").look(180.0F, 0.0F).hold(10, "w").hold(15, "wr").hold(10, "");
		s.now(recover());
		place(s, 8.5, Y0, 0.5, 90.0F).hold(13, "wr").hold(4, "w").look(-90.0F, 0.0F).hold(12, "w").hold(15, "wr").hold(10, "");
		s.now(recover());
		place(s, 0.5, Y0, 10.5, 180.0F).hold(8, "wr").hold(14, "wrj").look(0.0F, 0.0F).hold(15, "wr").hold(10, "");
		return s;
	}

	static Scenario waterClimbables() {
		Scenario s = Scenario.of("water_climbables", "Climbing in water: a waterlogged ladder up the wall of a 6-deep pool and a waterlogged scaffolding column (the climb rules against the fluid rules), sneaking, jumping and letting go.");
		s.fill(-4, Y0, -4, 4, Y0 + 5, 4, "minecraft:stone");
		s.fill(-3, Y0, -3, 3, Y0 + 5, 3, WATER);
		s.fill(0, Y0, -3, 0, Y0 + 5, -3, "minecraft:ladder[facing=south,waterlogged=true]");
		s.fill(2, Y0, 1, 2, Y0 + 4, 1, "minecraft:scaffolding[bottom=false,distance=0,waterlogged=true]");
		s.start(0.5, Y0, 0.5, 180.0F, 0.0F)
			.settle(60)
			.hold(10, "")
			.hold(35, "w")
			.hold(60, "w")
			.hold(25, "c")
			.hold(25, "")
			.hold(20, "j")
			.hold(30, "wj");
		place(s, 2.5, Y0, -1.5, 0.0F).hold(30, "w").hold(50, "j").hold(25, "c").hold(25, "").hold(20, "wj").hold(20, "");
		return s;
	}

	static Scenario sneakEdgeDrops() {
		Scenario s = Scenario.of("sneak_edge_drops", "Sneaking off the edge of a 1-block platform onto lower surfaces whose drop is just under or over the 0.6 the edge back-off tolerates: slab (0.5), snow layers 4 (0.625) and 5 (0.5), a daylight sensor (0.625), a trapdoor (0.8125), a bed (0.4375), carpet (0.9375) and soul sand (0.125).");
		String[] below = {
			SLAB_BOTTOM,
			"minecraft:snow[layers=4]",
			"minecraft:snow[layers=5]",
			"minecraft:daylight_detector[inverted=false,power=0]",
			TRAPDOOR_BOTTOM_CLOSED,
			"minecraft:red_bed[facing=south,occupied=false,part=foot]",
			"minecraft:white_carpet",
			"minecraft:soul_sand"};
		for (int i = 0; i < below.length; i++) {
			int x = -21 + 6 * i;
			s.fill(x - 1, Y0, -3, x + 1, Y0, -1, "minecraft:stone");
			s.fill(x - 1, Y0, 0, x + 1, Y0, 3, below[i]);
		}
		for (int i = 0; i < below.length; i++) {
			int x = -21 + 6 * i;
			if (i == 0) {
				s.start(x + 0.5, Y0 + 1, -1.0, 0.0F, 0.0F).hold(5, "");
			} else {
				place(s, x + 0.5, Y0 + 1, -1.0, 0.0F);
			}
			s.hold(36, "wc").hold(8, "w");
		}
		return s;
	}

	static Scenario effectsInWater() {
		Scenario s = Scenario.of("effects_in_water", "Status effects in a 4-deep pool: levitation lifting the player out of the water and into the air, slow falling and sinking, jump boost V swimming up, speed III and slowness IV swimming.");
		basin(s, 4, -4, 4, 4, WATER);
		s.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.settle(60)
			.hold(5, "")
			.now(setEffect("minecraft:levitation", 0, 100000))
			.hold(60, "")
			.hold(12, "w")
			.now(clearEffects())
			.hold(50, "")
			.now(setEffect("minecraft:slow_falling", 0, 100000))
			.hold(30, "")
			.hold(20, "c")
			.hold(15, "j")
			.now(setEffect("minecraft:jump_boost", 4, 100000))
			.hold(25, "j")
			.hold(25, "wj")
			.now(setEffect("minecraft:speed", 2, 100000))
			.hold(30, "wr")
			.hold(20, "w", 3.0F, 0.0F)
			.hold(15, "wc")
			.now(setEffect("minecraft:slowness", 3, 100000))
			.hold(35, "wr")
			.hold(15, "");
		return s;
	}

	// Four stretches of seeded random input, the later ones starting (by teleport) in or beside the
	// fluid and climbing features that a walker rarely reaches by chance: the deep pool, the upward
	// bubble column, the downward one and the foot of the ladder tower.
	static void courseRun(Course c, long seed, String[] moves, float turn, int minLen, int maxLen, int segments) {
		Scenario s = c.s;
		randomWalk(s, seed, segments, moves, turn, minLen, maxLen);
		double[][] spots = {{5.5, Y0 + 1, -0.5, -90.0}, {-7.5, Y0 + 1, -6.5, 0.0}, {-6.5, Y0 + 1, 7.0, 180.0}, {0.5, Y0, 6.5, 0.0}};
		for (int k = 0; k < spots.length; k++) {
			double x = c.mx > 0 ? spots[k][0] : 1.0 - spots[k][0];
			double z = c.mz > 0 ? spots[k][2] : 1.0 - spots[k][2];
			place(s, x, spots[k][1], z, (float) spots[k][3]);
			randomWalk(s, seed * 10 + k, segments, moves, turn, minLen, maxLen);
		}
	}

	static Scenario sprintStarve() {
		return Scenario.of("sprint_starve", "Sprint-jumping in a circle with food 7, no saturation and exhaustion near the threshold: food drops to 6 mid-run, at which point sprinting stops.")
			.food(7)
			.start(0.5, Y0, 0.5, 0.0F, 0.0F)
			.now((level, player, log) -> {
				player.getFoodData().setSaturation(0.0F);
				player.getFoodData().exhaustionLevel = 3.6F;
				log.addProperty("op", "starve");
			})
			.hold(10, "")
			.hold(200, "wrj", 3.0F, 0.0F)
			.hold(40, "wr", 3.0F, 0.0F)
			.hold(20, "");
	}

	static Scenario powderSnowFreeze() {
		return Scenario.of("powder_snow_freeze", "Standing in a powder snow pit for 200 ticks (freezing builds to its maximum and freeze damage starts), climbing out by jumping and thawing.")
			.fill(-2, Y0, -2, 2, Y0 + 1, 2, "minecraft:powder_snow")
			.start(0.5, Y0, -5.5, 0.0F, 0.0F)
			.hold(5, "")
			.hold(40, "w")
			.hold(200, "")
			.hold(40, "wj")
			.hold(20, "w")
			.hold(60, "");
	}

	// ---- wave 2 end (new scenarios are inserted above this line)
}
