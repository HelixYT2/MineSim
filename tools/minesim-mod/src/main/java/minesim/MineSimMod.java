package minesim;

import com.mojang.brigadier.Command;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
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

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

// Server-side: `/minesim dumpblocks` writes every block state's collision boxes and friction.
// Coordinates and friction are emitted as raw IEEE-754 bits so they round-trip exactly.
public class MineSimMod implements ModInitializer {
	@Override
	public void onInitialize() {
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

	private static Path dumpBlocks() throws IOException {
		StringBuilder sb = new StringBuilder("{\n");
		boolean first = true;
		for (Block block : BuiltInRegistries.BLOCK) {
			String name = BuiltInRegistries.BLOCK.getKey(block).toString();
			int friction = Float.floatToRawIntBits(block.getFriction());
			for (BlockState state : block.getStateDefinition().getPossibleStates()) {
				int id = Block.getId(state);
				VoxelShape shape = state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
				List<AABB> boxes = shape.toAabbs();
				if (!first) {
					sb.append(",\n");
				}
				first = false;
				sb.append("  \"").append(id).append("\":{\"block\":\"").append(name)
					.append("\",\"friction\":").append(friction).append(",\"aabbs\":[");
				for (int i = 0; i < boxes.size(); i++) {
					AABB b = boxes.get(i);
					if (i > 0) {
						sb.append(",");
					}
					sb.append("[")
						.append(Double.doubleToRawLongBits(b.minX)).append(",")
						.append(Double.doubleToRawLongBits(b.minY)).append(",")
						.append(Double.doubleToRawLongBits(b.minZ)).append(",")
						.append(Double.doubleToRawLongBits(b.maxX)).append(",")
						.append(Double.doubleToRawLongBits(b.maxY)).append(",")
						.append(Double.doubleToRawLongBits(b.maxZ)).append("]");
				}
				sb.append("]}");
			}
		}
		sb.append("\n}\n");
		Path out = FabricLoader.getInstance().getGameDir().resolve("minesim-blocks.json");
		Files.writeString(out, sb.toString());
		return out;
	}
}
