package minesim;

import com.google.gson.JsonObject;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.projectile.Projectile;

import java.io.BufferedWriter;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;

// Server-side per-tick trace of every living entity and projectile, written as JSON lines to
// minesim-trace-server.jsonl. The server is authoritative for projectiles and mobs, so this is the
// source of truth for projectile flight and knockback sources, complementing the client trace of
// the local player. One line per entity per server tick.
public final class ServerTracer {
	private static BufferedWriter writer;
	private static long tick;

	private ServerTracer() {
	}

	public static void register() {
		ServerTickEvents.END_SERVER_TICK.register(ServerTracer::onTick);
		ServerLifecycleEvents.SERVER_STOPPING.register(server -> close());
	}

	private static void onTick(MinecraftServer server) {
		try {
			BufferedWriter w = writer();
			for (ServerLevel level : server.getAllLevels()) {
				String dimension = level.dimension().toString();
				for (Entity e : level.getAllEntities()) {
					if (e instanceof Projectile || e instanceof LivingEntity) {
						JsonObject o = TraceJson.entity(e);
						o.addProperty("tick", tick);
						o.addProperty("dimension", dimension);
						w.write(TraceJson.GSON.toJson(o));
						w.write('\n');
					}
				}
			}
			w.flush();
			tick++;
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
	}

	private static BufferedWriter writer() throws IOException {
		if (writer == null) {
			Path out = FabricLoader.getInstance().getGameDir().resolve("minesim-trace-server.jsonl");
			writer = Files.newBufferedWriter(out);
		}
		return writer;
	}

	private static void close() {
		if (writer != null) {
			try {
				writer.close();
			} catch (IOException ignored) {
				// nothing useful to do while shutting down
			}
			writer = null;
		}
	}
}
