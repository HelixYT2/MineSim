package minesim.oracle;

import com.google.gson.JsonArray;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;

import java.util.List;
import java.util.Set;

// The fixed test arena every oracle scenario is built in: a box around the world origin on top of the
// superflat stone floor (the floor's top face is y = -63). Everything here runs on the server thread.
public final class OracleArena {
	public static final int FLOOR_Y = -64;
	public static final int MIN_X = -24;
	public static final int MAX_X = 24;
	public static final int MIN_Z = -24;
	public static final int MAX_Z = 24;
	public static final int MAX_Y = -24;

	// Place without neighbour or shape updates and without drops: what a scenario lists is exactly what
	// is in the world, so ladders stay unsupported-but-present and fences keep the given connections.
	private static final int PLACE_FLAGS = Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE | Block.UPDATE_SUPPRESS_DROPS;

	private OracleArena() {
	}

	// World-wide settings that remove every source of nondeterminism unrelated to the scenario.
	public static void configure(MinecraftServer server) {
		GameRules rules = server.overworld().getGameRules();
		rules.set(GameRules.ADVANCE_TIME, false, server);
		rules.set(GameRules.ADVANCE_WEATHER, false, server);
		rules.set(GameRules.RANDOM_TICK_SPEED, 0, server);
		rules.set(GameRules.SPAWN_MOBS, false, server);
		rules.set(GameRules.SPAWN_MONSTERS, false, server);
		rules.set(GameRules.SPAWN_PATROLS, false, server);
		rules.set(GameRules.SPAWN_PHANTOMS, false, server);
		rules.set(GameRules.SPAWN_WANDERING_TRADERS, false, server);
		rules.set(GameRules.SPAWN_WARDENS, false, server);
		rules.set(GameRules.MOB_GRIEFING, false, server);
		rules.set(GameRules.NATURAL_HEALTH_REGENERATION, false, server);
		rules.set(GameRules.PLAYER_MOVEMENT_CHECK, false, server);
		rules.set(GameRules.ELYTRA_MOVEMENT_CHECK, false, server);
		rules.set(GameRules.SEND_COMMAND_FEEDBACK, false, server);
		server.overworld().setDayTime(6000L);
		server.overworld().setWeatherParameters(6000, 0, false, false);
	}

	// Empty the arena (everything above the floor, the floor itself restored to stone) and remove every
	// non-player entity in it.
	public static void clear(ServerLevel level) {
		BlockState air = Blocks.AIR.defaultBlockState();
		BlockState stone = Blocks.STONE.defaultBlockState();
		BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
		for (int x = MIN_X; x <= MAX_X; x++) {
			for (int z = MIN_Z; z <= MAX_Z; z++) {
				p.set(x, FLOOR_Y, z);
				if (level.getBlockState(p) != stone) {
					level.setBlock(p, stone, PLACE_FLAGS);
				}
				for (int y = FLOOR_Y + 1; y <= MAX_Y; y++) {
					p.set(x, y, z);
					if (!level.getBlockState(p).isAir()) {
						level.setBlock(p, air, PLACE_FLAGS);
					}
				}
			}
		}
		AABB box = new AABB(MIN_X - 8, FLOOR_Y - 8, MIN_Z - 8, MAX_X + 8, MAX_Y + 64, MAX_Z + 8);
		for (Entity e : level.getEntities((Entity) null, box, e -> !(e instanceof Player))) {
			e.discard();
		}
	}

	public static BlockState parse(ServerLevel level, String state) {
		try {
			return BlockStateParser.parseForBlock(level.holderLookup(Registries.BLOCK), state, false).blockState();
		} catch (CommandSyntaxException e) {
			throw new IllegalArgumentException("bad block state '" + state + "': " + e.getMessage(), e);
		}
	}

	public static void fill(ServerLevel level, int x0, int y0, int z0, int x1, int y1, int z1, String state) {
		BlockState s = parse(level, state);
		for (int x = Math.min(x0, x1); x <= Math.max(x0, x1); x++) {
			for (int y = Math.min(y0, y1); y <= Math.max(y0, y1); y++) {
				for (int z = Math.min(z0, z1); z <= Math.max(z0, z1); z++) {
					checkInside(x, y, z);
					level.setBlock(new BlockPos(x, y, z), s, PLACE_FLAGS);
				}
			}
		}
	}

	private static void checkInside(int x, int y, int z) {
		if (x < MIN_X || x > MAX_X || z < MIN_Z || z > MAX_Z || y < FLOOR_Y || y > MAX_Y) {
			throw new IllegalArgumentException("block outside the oracle arena: " + x + "," + y + "," + z);
		}
	}

	// Every block in the arena that differs from the base world (stone floor, air above), as
	// [x, y, z, "block[state]"]. This is the world a replay is built from, so it is captured after the
	// scenario has settled (flowing water spread, etc.), not from the scenario's own block list.
	public static JsonArray dump(ServerLevel level) {
		JsonArray out = new JsonArray();
		BlockState stone = Blocks.STONE.defaultBlockState();
		BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
		for (int y = FLOOR_Y; y <= MAX_Y; y++) {
			for (int x = MIN_X; x <= MAX_X; x++) {
				for (int z = MIN_Z; z <= MAX_Z; z++) {
					p.set(x, y, z);
					BlockState s = level.getBlockState(p);
					boolean base = y == FLOOR_Y ? s == stone : s.isAir();
					if (!base) {
						JsonArray b = new JsonArray();
						b.add(x);
						b.add(y);
						b.add(z);
						b.add(BlockStateParser.serialize(s));
						out.add(b);
					}
				}
			}
		}
		return out;
	}

	// Put the player into the scenario's starting state: survival, full health and food, no effects,
	// no fire, at rest at the start position.
	public static void resetPlayer(ServerPlayer player, Vec3 pos, float yaw, float pitch, int food, float health) {
		ServerLevel level = player.level();
		player.setGameMode(GameType.SURVIVAL);
		player.getAbilities().flying = false;
		player.onUpdateAbilities();
		player.removeAllEffects();
		player.clearFire();
		player.setHealth(health);
		player.setAbsorptionAmount(0.0F);
		player.getFoodData().setFoodLevel(food);
		player.getFoodData().setSaturation(5.0F);
		player.getFoodData().exhaustionLevel = 0.0F;
		player.invulnerableTime = 0;
		player.hurtTime = 0;
		player.setDeltaMovement(Vec3.ZERO);
		player.teleportTo(level, pos.x, pos.y, pos.z, Set.of(), yaw, pitch, true);
		player.resetFallDistance();
		player.setDeltaMovement(Vec3.ZERO);
	}

	public static void addEffect(ServerPlayer player, String id, int amplifier, int duration) {
		Holder<MobEffect> effect = BuiltInRegistries.MOB_EFFECT.get(Identifier.parse(id))
			.orElseThrow(() -> new IllegalArgumentException("unknown effect " + id));
		player.addEffect(new MobEffectInstance(effect, duration, amplifier));
	}

	public static List<Entity> entitiesInArena(ServerLevel level) {
		AABB box = new AABB(MIN_X - 96, FLOOR_Y - 64, MIN_Z - 96, MAX_X + 96, MAX_Y + 256, MAX_Z + 96);
		return level.getEntities((Entity) null, box, e -> !(e instanceof Player));
	}
}
