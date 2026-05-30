package minesim.client;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.world.phys.Vec3;

import java.io.BufferedWriter;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;

// Client-side: appends one row per tick describing the local player's state and the keys held,
// so a walked session can be replayed through MineSim later. Doubles/floats are raw IEEE bits.
public class MineSimClient implements ClientModInitializer {
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
			Vec3 v = p.getDeltaMovement();
			Options o = client.options;
			writer().write(tick + ","
				+ Double.doubleToRawLongBits(p.getX()) + ","
				+ Double.doubleToRawLongBits(p.getY()) + ","
				+ Double.doubleToRawLongBits(p.getZ()) + ","
				+ Double.doubleToRawLongBits(v.x) + ","
				+ Double.doubleToRawLongBits(v.y) + ","
				+ Double.doubleToRawLongBits(v.z) + ","
				+ Float.floatToRawIntBits(p.getYRot()) + ","
				+ Float.floatToRawIntBits(p.getXRot()) + ","
				+ bit(p.onGround()) + ","
				+ bit(p.isSprinting()) + ","
				+ bit(p.isShiftKeyDown()) + ","
				+ bit(o.keyUp.isDown()) + ","
				+ bit(o.keyDown.isDown()) + ","
				+ bit(o.keyLeft.isDown()) + ","
				+ bit(o.keyRight.isDown()) + ","
				+ bit(o.keyJump.isDown()) + ","
				+ bit(o.keyShift.isDown()) + ","
				+ bit(o.keySprint.isDown()) + "\n");
			tick++;
		} catch (IOException e) {
			throw new UncheckedIOException(e);
		}
	}

	private static int bit(boolean b) {
		return b ? 1 : 0;
	}

	private BufferedWriter writer() throws IOException {
		if (writer == null) {
			Path out = FabricLoader.getInstance().getGameDir().resolve("minesim-trace.csv");
			writer = Files.newBufferedWriter(out);
			writer.write("tick,x,y,z,dx,dy,dz,yaw,pitch,onGround,sprinting,sneaking,"
				+ "keyUp,keyDown,keyLeft,keyRight,keyJump,keyShift,keySprint\n");
			writer.flush();
		}
		return writer;
	}
}
