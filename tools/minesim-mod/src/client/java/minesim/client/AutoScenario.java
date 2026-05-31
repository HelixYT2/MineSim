package minesim.client;

import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.client.player.LocalPlayer;

// Drives a fixed, deterministic capture routine once per launch: the player walks, sprints,
// sprint-jumps, sneaks and turns while status effects are applied, then stands still to be
// knocked about by a husk and to watch a volley of projectiles. Movement is driven by holding
// the real key bindings; the world actions are issued as commands (so the world needs cheats on).
// It runs at the start of the client tick, before movement is computed; the trace logger records
// the result at the end of the tick.
public final class AutoScenario {
	private static final int WARMUP_TICKS = 60;
	private static final int END_TICK = 700;

	private boolean started;
	private boolean finished;
	private int warmup = WARMUP_TICKS;
	private int t;

	public void tick(Minecraft client) {
		LocalPlayer p = client.player;
		if (finished || p == null || client.level == null) {
			return;
		}
		if (!started) {
			if (warmup-- > 0) {
				return;
			}
			started = true;
		}
		try {
			drive(client, p);
		} catch (RuntimeException e) {
			release(client.options);
			finished = true;
			return;
		}
		if (++t > END_TICK) {
			release(client.options);
			finished = true;
		}
	}

	private void drive(Minecraft client, LocalPlayer p) {
		Options o = client.options;
		if (t < 40) {
			hold(o, false, false, false, false); // settle on the ground
		} else if (t < 120) {
			hold(o, true, false, false, false); // walk
		} else if (t < 220) {
			hold(o, true, true, false, false); // sprint
		} else if (t < 360) {
			hold(o, true, true, true, false); // sprint-jump
		} else if (t < 420) {
			hold(o, true, false, false, true); // sneak
		} else if (t < 480) {
			hold(o, true, true, false, false); // sprint while turning
			p.setYRot(p.getYRot() + 2.0f);
		} else {
			hold(o, false, false, false, false); // stand still for knockback and projectiles
		}

		switch (t) {
			case 40 -> command(client, "effect give @s minecraft:speed 60 1");
			case 130 -> command(client, "effect give @s minecraft:jump_boost 60 1");
			case 300 -> command(client, "effect give @s minecraft:slow_falling 40 0");
			case 420 -> command(client, "effect give @s minecraft:slowness 30 0");
			case 478 -> command(client, "effect clear @s");
			case 485 -> command(client, "summon minecraft:husk ~ ~ ~1.5"); // husks ignore daylight
			case 560 -> command(client, "summon minecraft:snowball ~ ~1.5 ~ {Motion:[0.0,0.3,0.9]}");
			case 580 -> command(client, "summon minecraft:snowball ~ ~1.5 ~ {Motion:[0.6,0.2,0.6]}");
			case 600 -> command(client, "summon minecraft:arrow ~ ~1.6 ~ {Motion:[0.0,0.1,1.2]}");
			case 620 -> command(client, "summon minecraft:arrow ~ ~1.6 ~ {Motion:[0.4,0.4,0.4]}");
			case 640 -> command(client, "summon minecraft:snowball ~ ~1.5 ~ {Motion:[0.0,0.6,0.0]}");
			default -> {
				// no scripted action this tick
			}
		}
	}

	private static void hold(Options o, boolean forward, boolean sprint, boolean jump, boolean sneak) {
		o.keyUp.setDown(forward);
		o.keySprint.setDown(sprint);
		o.keyJump.setDown(jump);
		o.keyShift.setDown(sneak);
	}

	private static void release(Options o) {
		hold(o, false, false, false, false);
	}

	private static void command(Minecraft client, String command) {
		ClientPacketListener connection = client.getConnection();
		if (connection != null) {
			connection.sendCommand(command);
		}
	}
}
