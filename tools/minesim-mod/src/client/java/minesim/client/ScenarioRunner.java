package minesim.client;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import minesim.MineSimMod;
import minesim.Snapshot;
import minesim.TraceJson;
import minesim.oracle.OracleArena;
import minesim.oracle.OracleHooks;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.client.server.IntegratedServer;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.projectile.Projectile;
import net.minecraft.world.phys.Vec3;

import java.io.BufferedWriter;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicReference;

// Runs the oracle scenarios against the real client, one after another, and writes one trace file per
// scenario under the corpus directory. Enabled by -Dminesim.scenarios=all (or a comma list of names);
// with -Dminesim.quit=true the game exits when the last scenario finishes.
//
// For each scenario: the arena is cleared and built on the integrated server and the player reset,
// the world is left to settle, the final block layout is dumped, and then the input program is played.
// Every recorded tick logs the player state twice — at the start of the tick (after any packets from
// the server were applied, before physics) and at the end — so a replay sees exactly which changes came
// from outside the player's own tick (knockback, effects, health) and which from its physics.
public final class ScenarioRunner {
	private enum Phase { WAITING, BUILDING, SETTLING, DUMPING, RECORDING, DONE }

	private static final int WORLD_WARMUP_TICKS = 100;

	private final List<Scenario> queue;
	private final Path corpus;
	private final boolean quit;
	private final ConcurrentLinkedQueue<JsonObject> serverEvents = new ConcurrentLinkedQueue<>();
	private final AtomicReference<Object> serverResult = new AtomicReference<>();
	private final JsonArray manifest = new JsonArray();

	private Phase phase = Phase.WAITING;
	private int warmup = WORLD_WARMUP_TICKS;
	private int index = -1;
	private Scenario current;
	private int counter;
	private int t;
	private JsonObject lastPost;
	private JsonObject pre;
	private BufferedWriter out;
	private boolean dumped;

	private ScenarioRunner(List<Scenario> queue, Path corpus, boolean quit) {
		this.queue = queue;
		this.corpus = corpus;
		this.quit = quit;
	}

	// The runner configured by system properties, or null when scenario mode is off.
	public static ScenarioRunner fromSystemProperties() {
		String which = System.getProperty("minesim.scenarios");
		if (which == null || which.isBlank()) {
			return null;
		}
		List<Scenario> all = Scenarios.all();
		List<Scenario> picked = new ArrayList<>();
		if (which.equals("all")) {
			picked.addAll(all);
		} else {
			for (String name : which.split(",")) {
				Scenario s = all.stream().filter(x -> x.name.equals(name.trim())).findFirst()
					.orElseThrow(() -> new IllegalArgumentException("unknown scenario " + name));
				picked.add(s);
			}
		}
		Path dir = Path.of(System.getProperty("minesim.corpus",
			FabricLoader.getInstance().getGameDir().resolve("minesim-corpus").toString()));
		return new ScenarioRunner(picked, dir, Boolean.getBoolean("minesim.quit"));
	}

	public boolean active() {
		return phase != Phase.DONE;
	}

	// Start of a client tick: capture the pre-physics state and apply this tick's input.
	public void startTick(Minecraft client) {
		LocalPlayer p = client.player;
		if (p == null || client.level == null) {
			return;
		}
		if (phase != Phase.DONE && phase != Phase.RECORDING && p.isDeadOrDying()) {
			// A save left with a dead player (or a scenario that killed it): respawn and carry on.
			p.respawn();
			client.setScreen(null);
			return;
		}
		switch (phase) {
			case WAITING -> {
				release(client.options);
				if (client.screen == null && warmup-- <= 0) {
					begin(client);
				}
			}
			case BUILDING, SETTLING, DUMPING -> {
				release(client.options);
				p.setYRot(current.startYaw);
				p.setXRot(current.startPitch);
			}
			case RECORDING -> {
				pre = Snapshot.living(p);
				pre.addProperty("sprintTrigger", p.sprintTriggerTime);
				Scenario.Input in = current.input(t);
				Options o = client.options;
				o.keyUp.setDown(in.forward());
				o.keyDown.setDown(in.back());
				o.keyLeft.setDown(in.left());
				o.keyRight.setDown(in.right());
				o.keyJump.setDown(in.jump());
				o.keyShift.setDown(in.shift());
				o.keySprint.setDown(in.sprint());
				p.setYRot(in.yaw());
				p.setXRot(in.pitch());
				List<Scenario.ServerAction> acts = current.actions.get(t);
				if (acts != null) {
					schedule(client, (level, player) -> {
						for (Scenario.ServerAction a : acts) {
							JsonObject log = new JsonObject();
							log.addProperty("kind", "action");
							log.addProperty("serverTick", level.getServer().getTickCount());
							a.run(level, player, log);
							serverEvents.add(log);
						}
						return null;
					});
				}
			}
			default -> {
			}
		}
	}

	// End of a client tick: advance the state machine; while recording, write the trace row.
	public void endTick(Minecraft client) {
		LocalPlayer p = client.player;
		if (p == null || client.level == null) {
			return;
		}
		switch (phase) {
			case BUILDING -> {
				if (serverResult.get() instanceof RuntimeException) {
					serverResult.set(null);
					release(client.options);
					nextScenario(client);
				} else if (serverResult.get() != null) {
					serverResult.set(null);
					phase = Phase.SETTLING;
					counter = current.settle;
				}
			}
			case SETTLING -> {
				if (--counter <= 0) {
					phase = Phase.DUMPING;
					dumped = false;
					schedule(client, (level, player) -> OracleArena.dump(level));
				}
			}
			case DUMPING -> {
				Object r = serverResult.getAndSet(null);
				if (r instanceof RuntimeException) {
					release(client.options);
					nextScenario(client);
				} else if (r != null && !dumped) {
					dumped = true;
					openTrace((JsonArray) r, p);
					phase = Phase.RECORDING;
					t = 0;
				}
			}
			case RECORDING -> {
				JsonObject post = Snapshot.living(p);
				post.addProperty("sprintTrigger", p.sprintTriggerTime);
				JsonObject row = new JsonObject();
				row.addProperty("t", t);
				row.add("in", current.input(t).json());
				row.add("pre", lastPost == null ? pre : diff(lastPost, pre));
				row.add("post", post);
				JsonArray srv = new JsonArray();
				JsonObject ev;
				while ((ev = serverEvents.poll()) != null) {
					srv.add(ev);
				}
				if (!srv.isEmpty()) {
					row.add("srv", srv);
				}
				write(row);
				lastPost = post;
				if (++t >= current.ticks()) {
					finishScenario(client);
				}
			}
			default -> {
			}
		}
	}

	private void begin(Minecraft client) {
		schedule(client, (level, player) -> {
			OracleArena.configure(level.getServer());
			if (Boolean.getBoolean("minesim.dumpblocks")) {
				try {
					MineSimMod.dumpBlocks();
				} catch (IOException e) {
					throw new UncheckedIOException(e);
				}
			}
			return null;
		});
		serverResult.set(null);
		nextScenario(client);
	}

	private void nextScenario(Minecraft client) {
		index++;
		if (index >= queue.size()) {
			finishAll(client);
			return;
		}
		current = queue.get(index);
		lastPost = null;
		serverEvents.clear();
		OracleHooks.setServerTickListener(current.trackProjectiles ? this::sampleProjectiles : null);
		phase = Phase.BUILDING;
		Scenario s = current;
		schedule(client, (level, player) -> {
			OracleArena.clear(level);
			for (var op : s.build) {
				op.accept(level);
			}
			OracleArena.resetPlayer(player, new Vec3(s.startX, s.startY, s.startZ), s.startYaw, s.startPitch, s.food, s.health);
			for (String[] e : s.effects) {
				OracleArena.addEffect(player, e[0], Integer.parseInt(e[1]), Integer.parseInt(e[2]));
			}
			return Boolean.TRUE;
		});
	}

	private void sampleProjectiles(MinecraftServer server) {
		ServerLevel level = server.overworld();
		JsonArray arr = new JsonArray();
		for (Entity e : OracleArena.entitiesInArena(level)) {
			if (e instanceof Projectile proj) {
				arr.add(Snapshot.projectile(proj));
			}
		}
		if (!arr.isEmpty()) {
			JsonObject o = new JsonObject();
			o.addProperty("kind", "projectiles");
			o.addProperty("serverTick", server.getTickCount());
			o.add("entities", arr);
			serverEvents.add(o);
		}
	}

	private interface ServerTask {
		Object run(ServerLevel level, ServerPlayer player);
	}

	// Run `task` on the integrated server thread against the overworld and this client's server-side
	// player. A non-null return value is handed back through serverResult.
	private void schedule(Minecraft client, ServerTask task) {
		IntegratedServer server = client.getSingleplayerServer();
		if (server == null) {
			throw new IllegalStateException("the oracle needs a singleplayer world");
		}
		java.util.UUID id = client.player.getUUID();
		server.execute(() -> {
			ServerPlayer player = server.getPlayerList().getPlayer(id);
			try {
				Object r = task.run(server.overworld(), player);
				if (r != null) {
					serverResult.set(r);
				}
			} catch (RuntimeException e) {
				// A broken scenario must not wedge the whole capture: report it and let the runner skip it.
				System.err.println("[minesim] scenario '" + (current == null ? "?" : current.name) + "' failed on the server: " + e);
				e.printStackTrace();
				serverResult.set(e);
			}
		});
	}

	private void openTrace(JsonArray blocks, LocalPlayer p) {
		try {
			Path dir = corpus.resolve("client");
			Files.createDirectories(dir);
			out = Files.newBufferedWriter(dir.resolve(current.name + ".jsonl"));
			JsonObject header = current.header();
			header.addProperty("format", "minesim-oracle/1");
			header.addProperty("minecraft", "1.21.11");
			JsonObject arena = new JsonObject();
			arena.addProperty("floorY", OracleArena.FLOOR_Y);
			arena.addProperty("floorBlock", "minecraft:stone");
			arena.addProperty("minX", OracleArena.MIN_X);
			arena.addProperty("maxX", OracleArena.MAX_X);
			arena.addProperty("minZ", OracleArena.MIN_Z);
			arena.addProperty("maxZ", OracleArena.MAX_Z);
			arena.addProperty("maxY", OracleArena.MAX_Y);
			header.add("arena", arena);
			header.add("blocks", blocks);
			write(header);
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
	}

	private void write(JsonObject o) {
		try {
			out.write(TraceJson.GSON.toJson(o));
			out.write('\n');
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
	}

	private void finishScenario(Minecraft client) {
		try {
			out.close();
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
		out = null;
		JsonObject m = new JsonObject();
		m.addProperty("scenario", current.name);
		m.addProperty("ticks", current.ticks());
		manifest.add(m);
		release(client.options);
		nextScenario(client);
	}

	private void finishAll(Minecraft client) {
		phase = Phase.DONE;
		OracleHooks.setServerTickListener(null);
		release(client.options);
		try {
			Files.createDirectories(corpus);
			Files.writeString(corpus.resolve("client-manifest.json"), TraceJson.GSON.toJson(manifest));
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
		if (quit) {
			client.stop();
		}
	}

	// The members of `now` that differ from `before` (the previous tick's end state).
	private static JsonObject diff(JsonObject before, JsonObject now) {
		JsonObject d = new JsonObject();
		for (Map.Entry<String, JsonElement> e : now.entrySet()) {
			JsonElement b = before.get(e.getKey());
			if (b == null || !b.equals(e.getValue())) {
				d.add(e.getKey(), e.getValue());
			}
		}
		for (String k : before.keySet()) {
			if (!now.has(k)) {
				d.add(k, com.google.gson.JsonNull.INSTANCE);
			}
		}
		return d;
	}

	private static void release(Options o) {
		o.keyUp.setDown(false);
		o.keyDown.setDown(false);
		o.keyLeft.setDown(false);
		o.keyRight.setDown(false);
		o.keyJump.setDown(false);
		o.keyShift.setDown(false);
		o.keySprint.setDown(false);
	}
}
