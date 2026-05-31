package minesim;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.projectile.Projectile;
import net.minecraft.world.phys.Vec3;

// Shared serialisation for the trace logs. Every float/double is written as its raw IEEE-754 bit
// pattern (a long/int) so values round-trip into the simulator without any decimal loss.
public final class TraceJson {
	public static final Gson GSON = new Gson();

	private TraceJson() {
	}

	// The motion state common to every entity: identity, position, velocity, facing, ground
	// contact, and — for projectiles — the firing entity. Per-entity-kind extras are added by the
	// caller.
	public static JsonObject entity(Entity e) {
		JsonObject o = new JsonObject();
		o.addProperty("id", e.getId());
		o.addProperty("type", BuiltInRegistries.ENTITY_TYPE.getKey(e.getType()).toString());
		o.addProperty("x", Double.doubleToRawLongBits(e.getX()));
		o.addProperty("y", Double.doubleToRawLongBits(e.getY()));
		o.addProperty("z", Double.doubleToRawLongBits(e.getZ()));
		Vec3 v = e.getDeltaMovement();
		o.addProperty("dx", Double.doubleToRawLongBits(v.x));
		o.addProperty("dy", Double.doubleToRawLongBits(v.y));
		o.addProperty("dz", Double.doubleToRawLongBits(v.z));
		o.addProperty("yaw", Float.floatToRawIntBits(e.getYRot()));
		o.addProperty("pitch", Float.floatToRawIntBits(e.getXRot()));
		o.addProperty("onGround", e.onGround() ? 1 : 0);
		if (e instanceof Projectile proj) {
			o.addProperty("projectile", 1);
			Entity owner = proj.getOwner();
			o.addProperty("owner", owner == null ? -1 : owner.getId());
		}
		return o;
	}

	public static int bit(boolean b) {
		return b ? 1 : 0;
	}
}
