package minesim.client;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import minesim.TraceJson;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.tags.FluidTags;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.projectile.Projectile;

import java.io.BufferedWriter;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;

// Client-side per-tick trace of the local player and everything around it, written as JSON lines
// to minesim-trace.jsonl. The player is client-authoritative for movement, so this is the source
// of truth for movement, fluids/climbing, status effects, attributes, and the velocity response
// to knockback. One line per client tick.
public class MineSimClient implements ClientModInitializer {
	private static final double NEAR_RADIUS_SQR = 48.0 * 48.0;

	private BufferedWriter writer;
	private long tick;

	@Override
	public void onInitializeClient() {
		ClientTickEvents.END_CLIENT_TICK.register(this::onTick);
	}

	private void onTick(Minecraft client) {
		LocalPlayer p = client.player;
		if (p == null || client.level == null) {
			return;
		}
		try {
			JsonObject row = new JsonObject();
			row.addProperty("tick", tick);
			row.add("player", player(p));
			row.add("input", input(client.options));
			row.add("effects", effects(p));
			row.add("attributes", attributes(p));
			row.add("entities", nearbyEntities(client, p));

			BufferedWriter w = writer();
			w.write(TraceJson.GSON.toJson(row));
			w.write('\n');
			w.flush();
			tick++;
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
	}

	private static JsonObject player(LocalPlayer p) {
		JsonObject o = TraceJson.entity(p);
		o.addProperty("sprinting", TraceJson.bit(p.isSprinting()));
		o.addProperty("sneaking", TraceJson.bit(p.isShiftKeyDown()));
		o.addProperty("crouching", TraceJson.bit(p.isCrouching()));
		o.addProperty("swimming", TraceJson.bit(p.isSwimming()));
		o.addProperty("inWater", TraceJson.bit(p.isInWater()));
		o.addProperty("underWater", TraceJson.bit(p.isUnderWater()));
		o.addProperty("inLava", TraceJson.bit(p.isInLava()));
		o.addProperty("onClimbable", TraceJson.bit(p.onClimbable()));
		o.addProperty("fallFlying", TraceJson.bit(p.isFallFlying()));
		o.addProperty("pose", p.getPose().name());
		o.addProperty("health", Float.floatToRawIntBits(p.getHealth()));
		o.addProperty("waterHeight", Double.doubleToRawLongBits(p.getFluidHeight(FluidTags.WATER)));
		o.addProperty("lavaHeight", Double.doubleToRawLongBits(p.getFluidHeight(FluidTags.LAVA)));
		return o;
	}

	private static JsonObject input(Options o) {
		JsonObject in = new JsonObject();
		in.addProperty("keyUp", TraceJson.bit(o.keyUp.isDown()));
		in.addProperty("keyDown", TraceJson.bit(o.keyDown.isDown()));
		in.addProperty("keyLeft", TraceJson.bit(o.keyLeft.isDown()));
		in.addProperty("keyRight", TraceJson.bit(o.keyRight.isDown()));
		in.addProperty("keyJump", TraceJson.bit(o.keyJump.isDown()));
		in.addProperty("keyShift", TraceJson.bit(o.keyShift.isDown()));
		in.addProperty("keySprint", TraceJson.bit(o.keySprint.isDown()));
		return in;
	}

	private static JsonArray effects(LocalPlayer p) {
		JsonArray arr = new JsonArray();
		for (MobEffectInstance e : p.getActiveEffects()) {
			JsonObject o = new JsonObject();
			o.addProperty("id", BuiltInRegistries.MOB_EFFECT.getKey(e.getEffect().value()).toString());
			o.addProperty("amplifier", e.getAmplifier());
			o.addProperty("duration", e.getDuration());
			arr.add(o);
		}
		return arr;
	}

	// Every attribute the player actually has, keyed by its registry id, as raw double bits.
	// Iterating the registry rather than naming attributes keeps this stable across versions.
	private static JsonObject attributes(LocalPlayer p) {
		JsonObject attrs = new JsonObject();
		for (Attribute a : BuiltInRegistries.ATTRIBUTE) {
			Holder<Attribute> h = BuiltInRegistries.ATTRIBUTE.wrapAsHolder(a);
			if (p.getAttributes().hasAttribute(h)) {
				String key = BuiltInRegistries.ATTRIBUTE.getKey(a).toString();
				attrs.addProperty(key, Double.doubleToRawLongBits(p.getAttributeValue(h)));
			}
		}
		return attrs;
	}

	// Projectiles (wherever they are) plus anything within NEAR_RADIUS — enough to reconstruct
	// projectile flight and the source of any knockback.
	private static JsonArray nearbyEntities(Minecraft client, LocalPlayer p) {
		JsonArray arr = new JsonArray();
		for (Entity e : client.level.entitiesForRendering()) {
			if (e == p) {
				continue;
			}
			if (e instanceof Projectile || e.distanceToSqr(p) <= NEAR_RADIUS_SQR) {
				arr.add(TraceJson.entity(e));
			}
		}
		return arr;
	}

	private BufferedWriter writer() throws IOException {
		if (writer == null) {
			Path out = FabricLoader.getInstance().getGameDir().resolve("minesim-trace.jsonl");
			writer = Files.newBufferedWriter(out);
		}
		return writer;
	}
}
