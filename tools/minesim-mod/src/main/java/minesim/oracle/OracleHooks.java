package minesim.oracle;

import net.minecraft.server.MinecraftServer;

import java.util.function.Consumer;

// A single hook the oracle runner installs to observe the end of every server tick (projectile flight
// is server-side, so it has to be sampled there). Registered once from the common entrypoint.
public final class OracleHooks {
	private static volatile Consumer<MinecraftServer> serverTickListener;

	private OracleHooks() {
	}

	public static void setServerTickListener(Consumer<MinecraftServer> listener) {
		serverTickListener = listener;
	}

	public static void onServerTick(MinecraftServer server) {
		Consumer<MinecraftServer> l = serverTickListener;
		if (l != null) {
			l.accept(server);
		}
	}
}
