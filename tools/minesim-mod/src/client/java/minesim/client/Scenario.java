package minesim.client;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import minesim.oracle.OracleArena;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.function.Consumer;

// One oracle scenario: a block layout built in the arena, a starting state for the player, and a
// tick-by-tick input program (keys plus look direction), optionally with server-side actions such as
// knockback or projectile launches at given ticks. The runner builds it on the integrated server, lets
// the world settle, then records the real client player for every tick of the program.
public final class Scenario {
	// One tick of input: the seven movement keys and the absolute look direction.
	public record Input(boolean forward, boolean back, boolean left, boolean right, boolean jump, boolean shift,
		boolean sprint, float yaw, float pitch) {
		JsonObject json() {
			JsonObject o = new JsonObject();
			o.addProperty("f", forward ? 1 : 0);
			o.addProperty("b", back ? 1 : 0);
			o.addProperty("l", left ? 1 : 0);
			o.addProperty("r", right ? 1 : 0);
			o.addProperty("j", jump ? 1 : 0);
			o.addProperty("s", shift ? 1 : 0);
			o.addProperty("sp", sprint ? 1 : 0);
			o.addProperty("yaw", Float.floatToRawIntBits(yaw));
			o.addProperty("pitch", Float.floatToRawIntBits(pitch));
			return o;
		}
	}

	// Work done on the server thread at a scheduled tick. Whatever it records into `log` is attached to
	// the trace row of the client tick that follows.
	@FunctionalInterface
	public interface ServerAction {
		void run(ServerLevel level, ServerPlayer player, JsonObject log);
	}

	public final String name;
	public final String description;
	final List<Consumer<ServerLevel>> build = new ArrayList<>();
	final JsonArray buildLog = new JsonArray();
	final List<String[]> effects = new ArrayList<>();
	final List<Input> program = new ArrayList<>();
	final Map<Integer, List<ServerAction>> actions = new TreeMap<>();
	double startX = 0.5;
	double startY = -63.0;
	double startZ = 0.5;
	float startYaw;
	float startPitch;
	int food = 20;
	float health = 20.0F;
	int settle = 40;
	boolean trackProjectiles;

	private float yaw;
	private float pitch;

	private Scenario(String name, String description) {
		this.name = name;
		this.description = description;
	}

	public static Scenario of(String name, String description) {
		return new Scenario(name, description);
	}

	public Scenario fill(int x0, int y0, int z0, int x1, int y1, int z1, String state) {
		build.add(level -> OracleArena.fill(level, x0, y0, z0, x1, y1, z1, state));
		JsonArray op = new JsonArray();
		op.add(x0);
		op.add(y0);
		op.add(z0);
		op.add(x1);
		op.add(y1);
		op.add(z1);
		op.add(state);
		buildLog.add(op);
		return this;
	}

	public Scenario block(int x, int y, int z, String state) {
		return fill(x, y, z, x, y, z, state);
	}

	public Scenario start(double x, double y, double z, float yaw, float pitch) {
		this.startX = x;
		this.startY = y;
		this.startZ = z;
		this.startYaw = yaw;
		this.startPitch = pitch;
		this.yaw = yaw;
		this.pitch = pitch;
		return this;
	}

	public Scenario effect(String id, int amplifier) {
		return effect(id, amplifier, 100000);
	}

	public Scenario effect(String id, int amplifier, int duration) {
		effects.add(new String[] {id, Integer.toString(amplifier), Integer.toString(duration)});
		return this;
	}

	public Scenario food(int food) {
		this.food = food;
		return this;
	}

	public Scenario health(float health) {
		this.health = health;
		return this;
	}

	public Scenario settle(int ticks) {
		this.settle = ticks;
		return this;
	}

	public Scenario trackProjectiles() {
		this.trackProjectiles = true;
		return this;
	}

	// Hold `keys` for `ticks` ticks without turning. Keys: w forward, s back, a left, d right, j jump,
	// c sneak (shift), r sprint; anything else (e.g. "-" or "") means nothing pressed.
	public Scenario hold(int ticks, String keys) {
		return hold(ticks, keys, 0.0F, 0.0F);
	}

	// Hold `keys` while turning by `yawStep`/`pitchStep` degrees every tick.
	public Scenario hold(int ticks, String keys, float yawStep, float pitchStep) {
		for (int i = 0; i < ticks; i++) {
			yaw += yawStep;
			pitch += pitchStep;
			program.add(new Input(keys.indexOf('w') >= 0, keys.indexOf('s') >= 0, keys.indexOf('a') >= 0,
				keys.indexOf('d') >= 0, keys.indexOf('j') >= 0, keys.indexOf('c') >= 0, keys.indexOf('r') >= 0, yaw, pitch));
		}
		return this;
	}

	// Snap the look direction (takes effect from the next held tick).
	public Scenario look(float yaw, float pitch) {
		this.yaw = yaw;
		this.pitch = pitch;
		return this;
	}

	// Run `action` on the server at the start of client tick `tick` of the recording (0-based).
	public Scenario at(int tick, ServerAction action) {
		actions.computeIfAbsent(tick, k -> new ArrayList<>()).add(action);
		return this;
	}

	// Run `action` on the server when the program reaches its current length (i.e. "now" while building).
	public Scenario now(ServerAction action) {
		return at(program.size(), action);
	}

	public int ticks() {
		return program.size();
	}

	Input input(int t) {
		return program.get(t);
	}

	JsonObject header() {
		JsonObject o = new JsonObject();
		o.addProperty("scenario", name);
		o.addProperty("description", description);
		JsonObject start = new JsonObject();
		start.addProperty("x", Double.doubleToRawLongBits(startX));
		start.addProperty("y", Double.doubleToRawLongBits(startY));
		start.addProperty("z", Double.doubleToRawLongBits(startZ));
		start.addProperty("yaw", Float.floatToRawIntBits(startYaw));
		start.addProperty("pitch", Float.floatToRawIntBits(startPitch));
		start.addProperty("food", food);
		start.addProperty("health", Float.floatToRawIntBits(health));
		JsonArray eff = new JsonArray();
		for (String[] e : effects) {
			JsonObject eo = new JsonObject();
			eo.addProperty("id", e[0]);
			eo.addProperty("amp", Integer.parseInt(e[1]));
			eo.addProperty("dur", Integer.parseInt(e[2]));
			eff.add(eo);
		}
		start.add("effects", eff);
		o.add("start", start);
		o.add("build", buildLog);
		o.addProperty("ticks", program.size());
		o.addProperty("settle", settle);
		return o;
	}
}
