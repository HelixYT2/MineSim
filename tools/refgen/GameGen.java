import java.io.IOException;
import java.lang.reflect.Field;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import java.util.function.LongFunction;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomSupport;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;

// Generates reference vectors from the REAL Minecraft 1.21.11 classes (Mth, LegacyRandomSource,
// XoroshiroRandomSource, RandomSupport), so the Rust ports can be checked bit-for-bit:
//   crates/ms-numerics/data/mth_sin_table.bin      Mth.SIN, read from the class by reflection
//   crates/ms-numerics/data/mth_atan_tables.bin    Mth.ASIN_TAB then Mth.COS_TAB (little-endian doubles)
//   crates/ms-numerics/testdata/mth_reference.csv  op,a,b,c,result  (raw bits, unsigned decimal)
//   crates/ms-rng/testdata/legacy_random.csv       op,a,b,c,s,bits  (LegacyRandomSource sequences)
//   crates/ms-rng/testdata/xoroshiro_random.csv    op,a,b,c,s,bits  (XoroshiroRandomSource sequences)
//   crates/ms-rng/testdata/random_support.csv      op,a,b,c,s,bits  (RandomSupport, Mth.getSeed, String.hashCode)
// A mojmap-named game jar and the libraries from the server jar's META-INF/libraries must be on
// the classpath; NEVER commit any of them. See tools/refgen/README.md. Usage: java GameGen <repo-root>.
public final class GameGen {
    public static void main(String[] args) throws Exception {
        Path root = Paths.get(args.length > 0 ? args[0] : ".");
        writeTables(root);
        writeMth(root);
        writeRandom(root, "legacy_random.csv", LegacyRandomSource::new, false);
        writeRandom(root, "xoroshiro_random.csv", XoroshiroRandomSource::new, true);
        writeRandomSupport(root);
        System.out.println("game vectors written under " + root.toAbsolutePath());
    }

    // ------------------------------------------------------------------------------------------
    // Tables
    // ------------------------------------------------------------------------------------------

    private static Object staticField(Class<?> c, String name) throws Exception {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        return f.get(null);
    }

    private static void writeTables(Path root) throws Exception {
        Path dir = root.resolve("crates/ms-numerics/data");
        Files.createDirectories(dir);
        float[] sin = (float[]) staticField(Mth.class, "SIN");
        ByteBuffer buf = ByteBuffer.allocate(sin.length * 4).order(ByteOrder.LITTLE_ENDIAN);
        for (float v : sin) {
            buf.putInt(Float.floatToRawIntBits(v));
        }
        Files.write(dir.resolve("mth_sin_table.bin"), buf.array());

        double[] asin = (double[]) staticField(Mth.class, "ASIN_TAB");
        double[] cos = (double[]) staticField(Mth.class, "COS_TAB");
        ByteBuffer atan = ByteBuffer.allocate((asin.length + cos.length) * 8).order(ByteOrder.LITTLE_ENDIAN);
        for (double v : asin) {
            atan.putLong(Double.doubleToRawLongBits(v));
        }
        for (double v : cos) {
            atan.putLong(Double.doubleToRawLongBits(v));
        }
        Files.write(dir.resolve("mth_atan_tables.bin"), atan.array());
    }

    // ------------------------------------------------------------------------------------------
    // Mth
    // ------------------------------------------------------------------------------------------

    private static long fb(float f) {
        return Integer.toUnsignedLong(Float.floatToRawIntBits(f));
    }

    private static long db(double d) {
        return Double.doubleToRawLongBits(d);
    }

    private static long ib(int i) {
        return Integer.toUnsignedLong(i);
    }

    private static void mrow(StringBuilder o, String op, long a, long b, long c, long res) {
        o.append(op).append(',').append(Long.toUnsignedString(a)).append(',')
                .append(Long.toUnsignedString(b)).append(',').append(Long.toUnsignedString(c)).append(',')
                .append(Long.toUnsignedString(res)).append('\n');
    }

    private static double[] specialDoubles() {
        List<Double> v = new ArrayList<>();
        double[] base = { 0.0, -0.0, 0.5, -0.5, 1.0, -1.0, 1.5, -1.5, 2.5, -2.5, 179.99999, 180.0, -180.0,
                360.0, -360.0, 540.0, -540.0, 2147483647.0, 2147483647.5, 2147483648.0, -2147483648.0,
                -2147483648.5, -2147483649.0, 4294967296.0, 1e10, -1e10, 9.223372036854775807E18,
                -9.223372036854775808E18, 1e19, -1e19, Double.MIN_VALUE, -Double.MIN_VALUE, Double.MAX_VALUE,
                -Double.MAX_VALUE, Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY,
                16777216.0, 16777217.0, 0.99999999999999989, 1.0000000000000002 };
        for (double d : base) {
            v.add(d);
            v.add(Math.nextUp(d));
            v.add(Math.nextDown(d));
        }
        double[] out = new double[v.size()];
        for (int i = 0; i < out.length; i++) {
            out[i] = v.get(i);
        }
        return out;
    }

    private static double[] doubleInputs(Random r, int randomCount) {
        double[] sp = specialDoubles();
        double[] out = new double[sp.length + randomCount * 3];
        System.arraycopy(sp, 0, out, 0, sp.length);
        int k = sp.length;
        for (int i = 0; i < randomCount; i++) {
            out[k++] = (r.nextDouble() * 2 - 1) * Math.pow(10, r.nextInt(14) - 3);
            out[k++] = Math.rint((r.nextDouble() * 2 - 1) * 1000) + r.nextInt(3) * 0.25 - 0.25;
            out[k++] = Double.longBitsToDouble(r.nextLong());
        }
        return out;
    }

    private static float[] floatInputs(Random r, int randomCount) {
        List<Float> v = new ArrayList<>();
        for (double x : specialDoubles()) {
            float f = (float) x;
            v.add(f);
            v.add(Math.nextUp(f));
            v.add(Math.nextDown(f));
        }
        for (int i = 0; i < randomCount; i++) {
            v.add(Float.intBitsToFloat(r.nextInt()));
            v.add((r.nextFloat() * 2 - 1) * (float) Math.pow(10, r.nextInt(12) - 3));
            v.add((float) Math.rint((r.nextDouble() * 2 - 1) * 1000) + r.nextInt(3) * 0.25F - 0.25F);
        }
        float[] out = new float[v.size()];
        for (int i = 0; i < out.length; i++) {
            out[i] = v.get(i);
        }
        return out;
    }

    private static void writeMth(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-numerics/testdata");
        Files.createDirectories(dir);
        StringBuilder o = new StringBuilder("op,a,b,c,result\n");
        Random r = new Random(0x4D7468L);

        // ---- sin / cos with float arguments (widened to double by the call, exactly as the game does)
        List<Float> fa = new ArrayList<>();
        final float degToRad = (float) Math.PI / 180F; // the literal folding javac performs in the game's code
        for (int deg = -540; deg <= 540; deg++) {
            fa.add(deg * degToRad);
        }
        for (int deg = -1080; deg <= 1080; deg += 45) { // the quarter/eighth turns, e.g. -45 degrees
            fa.add(deg * degToRad);
            fa.add(deg * Mth.DEG_TO_RAD);
            fa.add(Math.nextUp(deg * degToRad));
            fa.add(Math.nextDown(deg * degToRad));
        }
        for (int i = 0; i < 700; i++) {
            fa.add((r.nextFloat() * 2 - 1) * 1000F * degToRad);   // yaw within +-1000 degrees
            fa.add((r.nextFloat() * 2 - 1) * (float) Math.pow(10, r.nextInt(8) - 3));
        }
        for (int i = 0; i <= 512; i++) {                           // every 128th table cell, in radians
            fa.add((float) (i * 128 / 10430.378350470453));
            fa.add((float) (-i * 128 / 10430.378350470453));
        }
        for (float f : new float[] { 0F, -0F, 1F, -1F, 0.5F, (float) Math.PI, (float) (Math.PI / 2),
                (float) (2 * Math.PI), 90F, 180F, 1e9F, -1e9F, 1e30F, Float.MAX_VALUE, -Float.MAX_VALUE,
                Float.MIN_VALUE, Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY }) {
            fa.add(f);
        }
        for (float f : fa) {
            mrow(o, "sin_f", fb(f), 0, 0, fb(Mth.sin((double) f)));
            mrow(o, "cos_f", fb(f), 0, 0, fb(Mth.cos((double) f)));
        }
        // ---- sin / cos with double arguments
        List<Double> da = new ArrayList<>();
        for (double d : doubleInputs(r, 300)) {
            da.add(d);
        }
        for (int i = 0; i < 65536; i += 257) {
            double c = i / 10430.378350470453;
            da.add(c);
            da.add(Math.nextUp(c));
            da.add(Math.nextDown(c));
            da.add(-c);
        }
        for (int i = 0; i < 300; i++) {
            da.add(Math.toRadians(r.nextInt(7201) / 10.0 - 360));
        }
        for (double d : da) {
            mrow(o, "sin_d", db(d), 0, 0, fb(Mth.sin(d)));
            mrow(o, "cos_d", db(d), 0, 0, fb(Mth.cos(d)));
        }

        // ---- sqrt
        for (float f : floatInputs(r, 100)) {
            mrow(o, "sqrt_f", fb(f), 0, 0, fb(Mth.sqrt(f)));
        }

        // ---- floor / ceil family
        double[] di = doubleInputs(r, 250);
        for (double d : di) {
            mrow(o, "floor_d", db(d), 0, 0, ib(Mth.floor(d)));
            mrow(o, "ceil_d", db(d), 0, 0, ib(Mth.ceil(d)));
            mrow(o, "lfloor", db(d), 0, 0, Mth.lfloor(d));
            mrow(o, "ceil_long", db(d), 0, 0, Mth.ceilLong(d));
            mrow(o, "frac_d", db(d), 0, 0, db(Mth.frac(d)));
            mrow(o, "wrap_d", db(d), 0, 0, db(Mth.wrapDegrees(d)));
        }
        float[] fi = floatInputs(r, 150);
        for (float f : fi) {
            mrow(o, "floor_f", fb(f), 0, 0, ib(Mth.floor(f)));
            mrow(o, "ceil_f", fb(f), 0, 0, ib(Mth.ceil(f)));
            mrow(o, "frac_f", fb(f), 0, 0, fb(Mth.frac(f)));
            mrow(o, "wrap_f", fb(f), 0, 0, fb(Mth.wrapDegrees(f)));
        }
        for (int i = 0; i < 500; i++) {
            int v = r.nextInt(5) == 0 ? r.nextInt() : r.nextInt(2001) - 1000;
            mrow(o, "wrap_i", ib(v), 0, 0, ib(Mth.wrapDegrees(v)));
            long l = r.nextInt(5) == 0 ? r.nextLong() : r.nextInt(100001) - 50000;
            mrow(o, "wrap_l", l, 0, 0, fb(Mth.wrapDegrees(l)));
        }
        for (int v : new int[] { 0, 179, 180, 181, -179, -180, -181, 360, -360, 539, 540, Integer.MAX_VALUE,
                Integer.MIN_VALUE }) {
            mrow(o, "wrap_i", ib(v), 0, 0, ib(Mth.wrapDegrees(v)));
            mrow(o, "wrap_l", v, 0, 0, fb(Mth.wrapDegrees((long) v)));
        }
        for (long l : new long[] { Long.MAX_VALUE, Long.MIN_VALUE, 1L << 40, -(1L << 40) }) {
            mrow(o, "wrap_l", l, 0, 0, fb(Mth.wrapDegrees(l)));
        }

        // ---- Math.min / Math.max (Java NaN and signed-zero rules)
        for (int i = 0; i < 600; i++) {
            float a = pick(r, fi), b = pick(r, fi);
            mrow(o, "min_f", fb(a), fb(b), 0, fb(Math.min(a, b)));
            mrow(o, "max_f", fb(a), fb(b), 0, fb(Math.max(a, b)));
            double x = pick(r, di), y = pick(r, di);
            mrow(o, "min_d", db(x), db(y), 0, db(Math.min(x, y)));
            mrow(o, "max_d", db(x), db(y), 0, db(Math.max(x, y)));
        }
        float[] zeros = { 0F, -0F, Float.NaN, 1F, -1F };
        for (float a : zeros) {
            for (float b : zeros) {
                mrow(o, "min_f", fb(a), fb(b), 0, fb(Math.min(a, b)));
                mrow(o, "max_f", fb(a), fb(b), 0, fb(Math.max(a, b)));
                mrow(o, "min_d", db(a), db(b), 0, db(Math.min((double) a, (double) b)));
                mrow(o, "max_d", db(a), db(b), 0, db(Math.max((double) a, (double) b)));
            }
        }

        // ---- clamp
        for (int i = 0; i < 700; i++) {
            float a = pick(r, fi), b = pick(r, fi), c = pick(r, fi);
            mrow(o, "clamp_f", fb(a), fb(b), fb(c), fb(Mth.clamp(a, b, c)));
            double x = pick(r, di), y = pick(r, di), z = pick(r, di);
            mrow(o, "clamp_d", db(x), db(y), db(z), db(Mth.clamp(x, y, z)));
            int p = r.nextInt(4) == 0 ? r.nextInt() : r.nextInt(41) - 20;
            int q = r.nextInt(4) == 0 ? r.nextInt() : r.nextInt(41) - 20;
            int s = r.nextInt(4) == 0 ? r.nextInt() : r.nextInt(41) - 20;
            mrow(o, "clamp_i", ib(p), ib(q), ib(s), ib(Mth.clamp(p, q, s)));
            long lp = r.nextLong() >> r.nextInt(64), lq = r.nextLong() >> r.nextInt(64), ls = r.nextLong() >> r.nextInt(64);
            mrow(o, "clamp_l", lp, lq, ls, Mth.clamp(lp, lq, ls));
        }
        for (float a : zeros) {
            for (float b : zeros) {
                for (float c : zeros) {
                    mrow(o, "clamp_f", fb(a), fb(b), fb(c), fb(Mth.clamp(a, b, c)));
                    mrow(o, "clamp_d", db(a), db(b), db(c), db(Mth.clamp((double) a, (double) b, (double) c)));
                }
            }
        }

        // ---- angle helpers and interpolation (floats)
        for (int i = 0; i < 500; i++) {
            float a = r.nextInt(4) == 0 ? pick(r, fi) : (r.nextFloat() * 2 - 1) * 720F;
            float b = r.nextInt(4) == 0 ? pick(r, fi) : (r.nextFloat() * 2 - 1) * 720F;
            float c = r.nextInt(4) == 0 ? pick(r, fi) : r.nextFloat() * 90F;
            mrow(o, "deg_diff", fb(a), fb(b), 0, fb(Mth.degreesDifference(a, b)));
            mrow(o, "rot_lerp_f", fb(c / 90F), fb(a), fb(b), fb(Mth.rotLerp(c / 90F, a, b)));
            mrow(o, "approach", fb(a), fb(b), fb(c), fb(Mth.approach(a, b, c)));
            mrow(o, "approach_deg", fb(a), fb(b), fb(c), fb(Mth.approachDegrees(a, b, c)));
            mrow(o, "lerp_f", fb(c / 90F), fb(a), fb(b), fb(Mth.lerp(c / 90F, a, b)));
            mrow(o, "inv_lerp_f", fb(a), fb(b), fb(c), fb(Mth.inverseLerp(a, b, c)));
            mrow(o, "pmod_f", fb(a), fb(b), 0, fb(Mth.positiveModulo(a, b)));
            double x = r.nextInt(4) == 0 ? pick(r, di) : (r.nextDouble() * 2 - 1) * 720.0;
            double y = r.nextInt(4) == 0 ? pick(r, di) : (r.nextDouble() * 2 - 1) * 720.0;
            double z = r.nextInt(4) == 0 ? pick(r, di) : r.nextDouble();
            mrow(o, "rot_lerp_d", db(z), db(x), db(y), db(Mth.rotLerp(z, x, y)));
            mrow(o, "lerp_d", db(z), db(x), db(y), db(Mth.lerp(z, x, y)));
            mrow(o, "inv_lerp_d", db(x), db(y), db(z), db(Mth.inverseLerp(x, y, z)));
            mrow(o, "pmod_d", db(x), db(y), 0, db(Mth.positiveModulo(x, y)));
        }
        for (int i = 0; i < 500; i++) {
            int a = r.nextInt(3) == 0 ? r.nextInt() : r.nextInt(2001) - 1000;
            int b = r.nextInt(3) == 0 ? r.nextInt() : r.nextInt(41) - 20;
            if (b == 0) {
                b = 7;
            }
            mrow(o, "pmod_i", ib(a), ib(b), 0, ib(Mth.positiveModulo(a, b)));
        }
        mrow(o, "pmod_i", ib(Integer.MIN_VALUE), ib(-1), 0, ib(Mth.positiveModulo(Integer.MIN_VALUE, -1)));

        // ---- fast_inv_sqrt and the table-driven Mth.atan2
        for (int i = 0; i < 800; i++) {
            double d = Math.abs(r.nextInt(3) == 0 ? pick(r, di) : (r.nextDouble() * 2 - 1) * Math.pow(10, r.nextInt(10) - 4));
            mrow(o, "fast_inv_sqrt", db(d), 0, 0, db(Mth.fastInvSqrt(d)));
        }
        double[] special = { 0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 1e-300, -1e-300, 1e300, -1e300,
                Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY, Double.MIN_VALUE, Double.MAX_VALUE };
        for (double y : special) {
            for (double x : special) {
                atan2Row(o, y, x);
            }
        }
        for (int i = 0; i < 2500; i++) {
            double y = (r.nextDouble() * 2 - 1) * Math.pow(10, r.nextInt(8) - 3);
            double x = (r.nextDouble() * 2 - 1) * Math.pow(10, r.nextInt(8) - 3);
            atan2Row(o, y, x);
        }
        for (int i = 0; i < 1500; i++) { // velocity-like vectors: rotation of arrows and mobs
            double ang = r.nextDouble() * 2 * Math.PI, m = 0.01 + r.nextDouble() * 5;
            atan2Row(o, m * Math.sin(ang), m * Math.cos(ang));
        }
        for (int i = 0; i < 150; i++) {
            double x = (r.nextDouble() * 2 - 1) * 4;
            atan2Row(o, 0.0, x);
            atan2Row(o, x, 0.0);
            atan2Row(o, x, x);
            atan2Row(o, -x, x);
        }
        for (int i = 0; i < 300; i++) {
            atan2Row(o, Double.longBitsToDouble(r.nextLong()), Double.longBitsToDouble(r.nextLong()));
        }

        // ---- Mth.getSeed
        for (int i = 0; i < 1000; i++) {
            int x = r.nextInt(4) == 0 ? r.nextInt() : r.nextInt(2001) - 1000;
            int y = r.nextInt(4) == 0 ? r.nextInt() : r.nextInt(401) - 64;
            int z = r.nextInt(4) == 0 ? r.nextInt() : r.nextInt(2001) - 1000;
            mrow(o, "get_seed", ib(x), ib(y), ib(z), Mth.getSeed(x, y, z));
        }
        Files.writeString(dir.resolve("mth_reference.csv"), o.toString());
    }

    private static void atan2Row(StringBuilder o, double y, double x) {
        mrow(o, "atan2", db(y), db(x), 0, db(Mth.atan2(y, x)));
    }

    private static float pick(Random r, float[] a) {
        return a[r.nextInt(a.length)];
    }

    private static double pick(Random r, double[] a) {
        return a[r.nextInt(a.length)];
    }

    // ------------------------------------------------------------------------------------------
    // RandomSource implementations
    // ------------------------------------------------------------------------------------------

    private static String hex(String s) {
        StringBuilder b = new StringBuilder();
        for (byte x : s.getBytes(StandardCharsets.UTF_8)) {
            b.append(String.format("%02x", x & 0xff));
        }
        return b.toString();
    }

    private static void rrow(StringBuilder o, String op, long a, long b, long c, String s, long bits) {
        o.append(op).append(',').append(a).append(',').append(b).append(',').append(c).append(',')
                .append(hex(s)).append(',').append(Long.toUnsignedString(bits)).append('\n');
    }

    private static final String[] HASH_NAMES = { "", "a", "minecraft:overworld", "minecraft:nether",
            "minecraft:terrain", "minecraft:offset", "Hello, World!", "café", "üñî",
            "日本語", "emoji 😀 pair", "minecraft:bamboo_jungle/surface" };

    private static final long[] SEEDS = { 0L, 1L, -1L, 2L, 42L, 25214903917L, 123456789L, -987654321L,
            Long.MIN_VALUE, Long.MAX_VALUE, 0xDEADBEEFL, 281474976710655L, 281474976710656L,
            0x9E3779B97F4A7C15L, 0x7640891576956012L };

    private static final int[] BOUNDS = { 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 100, 255, 256, 1000, 1024, 4095,
            65536, 0x3FFFFFFF, 0x40000000, 0x40000001, 999999999, 0x7FFFFFFF, 3000000, 0x55555555 };

    private static void writeRandom(Path root, String file, LongFunction<RandomSource> make, boolean xoro)
            throws IOException {
        Path dir = root.resolve("crates/ms-rng/testdata");
        Files.createDirectories(dir);
        StringBuilder o = new StringBuilder("op,a,b,c,s,bits\n");
        Random pick = new Random(xoro ? 0x786F72L : 0x6C6567L);
        List<Long> seeds = new ArrayList<>();
        for (long s : SEEDS) {
            seeds.add(s);
        }
        for (int i = 0; i < 40; i++) {
            seeds.add(pick.nextLong());
        }
        for (int i = 0; i < 12; i++) {
            seeds.add((long) pick.nextInt(1000)); // small, human seeds
        }
        for (long seed : seeds) {
            RandomSource rs = make.apply(seed);
            rrow(o, "new", seed, 0, 0, "", 0);
            RandomSource child = null;
            PositionalRandomFactory factory = null;
            int n = 70;
            for (int step = 0; step < n; step++) {
                int kind = pick.nextInt(24);
                switch (kind) {
                    case 0, 1 -> rrow(o, "int", 0, 0, 0, "", ib(rs.nextInt()));
                    case 2, 3, 4 -> {
                        int b = pick.nextInt(3) == 0 ? BOUNDS[pick.nextInt(BOUNDS.length)] : 1 + pick.nextInt(200);
                        rrow(o, "intb", b, 0, 0, "", ib(rs.nextInt(b)));
                    }
                    case 5 -> {
                        int lo = pick.nextInt(200) - 100, hi = lo + pick.nextInt(100);
                        rrow(o, "intbi", lo, hi, 0, "", ib(rs.nextIntBetweenInclusive(lo, hi)));
                    }
                    case 6 -> {
                        int lo = pick.nextInt(2000) - 1000, hi = lo + 1 + pick.nextInt(1000);
                        rrow(o, "intr", lo, hi, 0, "", ib(rs.nextInt(lo, hi)));
                    }
                    case 7, 8 -> rrow(o, "long", 0, 0, 0, "", rs.nextLong());
                    case 9, 10 -> rrow(o, "bool", 0, 0, 0, "", rs.nextBoolean() ? 1 : 0);
                    case 11, 12 -> rrow(o, "float", 0, 0, 0, "", fb(rs.nextFloat()));
                    case 13, 14 -> rrow(o, "double", 0, 0, 0, "", db(rs.nextDouble()));
                    case 15, 16, 17 -> rrow(o, "gauss", 0, 0, 0, "", db(rs.nextGaussian()));
                    case 18 -> {
                        double mode = (pick.nextDouble() - 0.5) * 10, dev = pick.nextDouble() * 3;
                        rrow(o, "tri_d", db(mode), db(dev), 0, "", db(rs.triangle(mode, dev)));
                    }
                    case 19 -> {
                        float mode = (pick.nextFloat() - 0.5F) * 10F, dev = pick.nextFloat() * 3F;
                        rrow(o, "tri_f", fb(mode), fb(dev), 0, "", fb(rs.triangle(mode, dev)));
                    }
                    case 20 -> {
                        int cnt = pick.nextInt(9) == 0 ? -2 : pick.nextInt(6);
                        rs.consumeCount(cnt);
                        rrow(o, "consume", cnt, 0, 0, "", 0);
                    }
                    case 21 -> {
                        child = rs.fork();
                        rrow(o, "fork", 0, 0, 0, "", child.nextLong());
                        rrow(o, "child_double", 0, 0, 0, "", db(child.nextDouble()));
                        rrow(o, "child_gauss", 0, 0, 0, "", db(child.nextGaussian()));
                    }
                    case 22 -> {
                        factory = rs.forkPositional();
                        rrow(o, "fp_new", 0, 0, 0, "", 0);
                        for (int i = 0; i < 3; i++) {
                            int x = pick.nextInt(2001) - 1000, y = pick.nextInt(400) - 64, z = pick.nextInt(2001) - 1000;
                            rrow(o, "fp_at", x, y, z, "", factory.at(x, y, z).nextLong());
                        }
                        String name = HASH_NAMES[pick.nextInt(HASH_NAMES.length)];
                        rrow(o, "fp_hash", 0, 0, 0, name, factory.fromHashOf(name).nextLong());
                        long fs = pick.nextLong();
                        rrow(o, "fp_seed", fs, 0, 0, "", factory.fromSeed(fs).nextLong());
                    }
                    default -> {
                        long ns = pick.nextInt(3) == 0 ? SEEDS[pick.nextInt(SEEDS.length)] : pick.nextLong();
                        rs.setSeed(ns);
                        rrow(o, "setseed", ns, 0, 0, "", 0);
                    }
                }
            }
            // a tail of consecutive gaussians shows the pair caching
            for (int i = 0; i < 5; i++) {
                rrow(o, "gauss", 0, 0, 0, "", db(rs.nextGaussian()));
            }
            // the gaussian cache is cleared by setSeed even when one value is pending
            rs.nextGaussian();
            rs.setSeed(seed ^ 0x123456789L);
            rrow(o, "setseed", seed ^ 0x123456789L, 0, 0, "", 0);
            rrow(o, "gauss", 0, 0, 0, "", db(rs.nextGaussian()));
            rrow(o, "gauss", 0, 0, 0, "", db(rs.nextGaussian()));
            rrow(o, "gauss", 0, 0, 0, "", db(rs.nextGaussian()));
        }
        if (xoro) {
            // two-long constructor (including the all-zero fallback) and the raw generator
            long[][] pairs = { { 0L, 0L }, { 1L, 0L }, { 0L, 1L }, { -1L, -1L }, { Long.MIN_VALUE, Long.MAX_VALUE },
                    { 0x9E3779B97F4A7C15L, 0x7640891576956012L }, { 123456789L, -987654321L } };
            List<long[]> all = new ArrayList<>(List.of(pairs));
            for (int i = 0; i < 20; i++) {
                all.add(new long[] { pick.nextLong(), pick.nextLong() });
            }
            for (long[] p : all) {
                XoroshiroRandomSource x = new XoroshiroRandomSource(p[0], p[1]);
                rrow(o, "new128", p[0], p[1], 0, "", 0);
                for (int i = 0; i < 24; i++) {
                    rrow(o, "long", 0, 0, 0, "", x.nextLong());
                }
                rrow(o, "int", 0, 0, 0, "", ib(x.nextInt()));
                rrow(o, "double", 0, 0, 0, "", db(x.nextDouble()));
                rrow(o, "gauss", 0, 0, 0, "", db(x.nextGaussian()));
            }
        }
        Files.writeString(dir.resolve(file), o.toString());
    }

    private static void writeRandomSupport(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-rng/testdata");
        Files.createDirectories(dir);
        StringBuilder o = new StringBuilder("op,a,b,c,s,bits\n");
        Random r = new Random(0x5355L);
        List<Long> vals = new ArrayList<>();
        for (long s : SEEDS) {
            vals.add(s);
        }
        for (int i = 0; i < 300; i++) {
            vals.add(r.nextLong());
        }
        for (long v : vals) {
            rrow(o, "mix13", v, 0, 0, "", RandomSupport.mixStafford13(v));
            RandomSupport.Seed128bit un = RandomSupport.upgradeSeedTo128bitUnmixed(v);
            rrow(o, "up128u_lo", v, 0, 0, "", un.seedLo());
            rrow(o, "up128u_hi", v, 0, 0, "", un.seedHi());
            RandomSupport.Seed128bit mx = RandomSupport.upgradeSeedTo128bit(v);
            rrow(o, "up128_lo", v, 0, 0, "", mx.seedLo());
            rrow(o, "up128_hi", v, 0, 0, "", mx.seedHi());
        }
        List<String> names = new ArrayList<>(List.of(HASH_NAMES));
        names.add("a".repeat(55));
        names.add("a".repeat(56));
        names.add("a".repeat(63));
        names.add("a".repeat(64));
        names.add("a".repeat(65));
        names.add("a".repeat(119));
        names.add("a".repeat(120));
        names.add("a".repeat(1000));
        for (int len = 1; len <= 130; len++) {
            names.add(randomString(r, len));
        }
        for (String s : names) {
            RandomSupport.Seed128bit h = RandomSupport.seedFromHashOf(s);
            rrow(o, "md5_lo", 0, 0, 0, s, h.seedLo());
            rrow(o, "md5_hi", 0, 0, 0, s, h.seedHi());
            rrow(o, "jhash", 0, 0, 0, s, ib(s.hashCode()));
        }
        Files.writeString(dir.resolve("random_support.csv"), o.toString());
    }

    private static String randomString(Random r, int len) {
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < len; i++) {
            int kind = r.nextInt(10);
            if (kind < 7) {
                b.append((char) ('a' + r.nextInt(26)));
            } else if (kind < 8) {
                b.append(":/_.-".charAt(r.nextInt(5)));
            } else if (kind < 9) {
                b.append((char) (0x80 + r.nextInt(0x700)));
            } else {
                b.appendCodePoint(0x10000 + r.nextInt(0x1000));
            }
        }
        return b.toString();
    }
}
