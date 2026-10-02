package minesim;

import com.mojang.brigadier.Command;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.Component;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;
import net.minecraft.world.level.material.FlowingFluid;
import net.minecraft.world.level.material.FluidState;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import minesim.oracle.OracleHooks;

// Server-side: `/minesim dumpblocks` writes every block state's collision boxes, fluid, and the
// per-block friction, speed/jump factors and tags.
// Coordinates and friction are emitted as raw IEEE-754 bits so they round-trip exactly.
public class MineSimMod implements ModInitializer {
	@Override
	public void onInitialize() {
		ServerTracer.register();
		ServerTickEvents.END_SERVER_TICK.register(OracleHooks::onServerTick);
		CommandRegistrationCallback.EVENT.register((dispatcher, access, env) ->
			dispatcher.register(Commands.literal("minesim")
				.then(Commands.literal("dumpblocks").executes(ctx -> {
					try {
						Path out = dumpBlocks();
						ctx.getSource().sendSuccess(() -> Component.literal("minesim: wrote " + out), false);
					} catch (IOException e) {
						ctx.getSource().sendFailure(Component.literal("minesim dump failed: " + e));
					}
					return Command.SINGLE_SUCCESS;
				}))));
	}

	// Per block: registry id, implementing class, friction/speed/jump factors and tags. Per state: the
	// context-free collision boxes, the fluid it carries and its suffocation flag. Floats and doubles
	// are raw IEEE-754 bits so they round-trip exactly.
	public static Path dumpBlocks() throws IOException {
		JsonObject root = new JsonObject();
		for (Block block : BuiltInRegistries.BLOCK) {
			String name = BuiltInRegistries.BLOCK.getKey(block).toString();
			JsonArray tags = new JsonArray();
			block.defaultBlockState().getTags().map(t -> t.location().toString()).sorted().forEach(tags::add);
			for (BlockState state : block.getStateDefinition().getPossibleStates()) {
				JsonObject o = new JsonObject();
				o.addProperty("block", name);
				o.addProperty("class", block.getClass().getSimpleName());
				o.addProperty("friction", Float.floatToRawIntBits(block.getFriction()));
				o.addProperty("speedFactor", Float.floatToRawIntBits(block.getSpeedFactor()));
				o.addProperty("jumpFactor", Float.floatToRawIntBits(block.getJumpFactor()));
				o.add("tags", tags);
				o.addProperty("suffocating", state.isSuffocating(EmptyBlockGetter.INSTANCE, BlockPos.ZERO) ? 1 : 0);
				FluidState fluid = state.getFluidState();
				if (!fluid.isEmpty()) {
					JsonObject fo = new JsonObject();
					fo.addProperty("type", BuiltInRegistries.FLUID.getKey(fluid.getType()).toString());
					fo.addProperty("amount", fluid.getAmount());
					fo.addProperty("source", fluid.isSource() ? 1 : 0);
					fo.addProperty("falling", fluid.hasProperty(FlowingFluid.FALLING) && fluid.getValue(FlowingFluid.FALLING) ? 1 : 0);
					fo.addProperty("ownHeight", Float.floatToRawIntBits(fluid.getOwnHeight()));
					o.add("fluid", fo);
				}
				JsonArray aabbs = new JsonArray();
				VoxelShape shape = state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
				for (AABB b : shape.toAabbs()) {
					JsonArray box = new JsonArray();
					box.add(Double.doubleToRawLongBits(b.minX));
					box.add(Double.doubleToRawLongBits(b.minY));
					box.add(Double.doubleToRawLongBits(b.minZ));
					box.add(Double.doubleToRawLongBits(b.maxX));
					box.add(Double.doubleToRawLongBits(b.maxY));
					box.add(Double.doubleToRawLongBits(b.maxZ));
					aabbs.add(box);
				}
				o.add("aabbs", aabbs);
				root.add(Integer.toString(Block.getId(state)), o);
			}
		}
		Path out = FabricLoader.getInstance().getGameDir().resolve("minesim-blocks.json");
		Files.writeString(out, TraceJson.GSON.toJson(root));
		return out;
	}
}
