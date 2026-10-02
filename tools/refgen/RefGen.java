import java.io.IOException;
import java.math.BigDecimal;
import java.math.MathContext;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Random;

// Generates reference vectors straight from the JVM (no Minecraft jar needed), so the Rust ports
// can be checked bit-for-bit:
//   * java.util.Random sequences                      -> crates/ms-rng/testdata/java_random.csv
//   * StrictMath (fdlibm) acos / atan / atan2 / log   -> crates/ms-numerics/testdata/fdlibm_reference.csv
//   * Math.log (HotSpot intrinsic) + correctly rounded
//     reference values                                -> crates/ms-numerics/testdata/hotspot_log.csv
// The vectors that need the game's own classes are produced by GameGen.java.
// See tools/refgen/README.md. Usage: java RefGen <repo-root>.
public final class RefGen {
    public static void main(String[] args) throws IOException {
        Path root = Paths.get(args.length > 0 ? args[0] : ".");
        writeJavaRandom(root);
        writeFdlibm(root);
        writeHotspotLog(root);
        System.out.println("reference data written under " + root.toAbsolutePath());
    }

    // ------------------------------------------------------------------------------------------
    // java.util.Random
    // ------------------------------------------------------------------------------------------

    private static void writeJavaRandom(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-rng/testdata");
        Files.createDirectories(dir);
        StringBuilder out = new StringBuilder("seed,op,arg,bits\n");
        java.util.ArrayList<Long> seeds = new java.util.ArrayList<>(java.util.List.of(
                0L, 1L, -1L, 42L, 25214903917L, 123456789L, -987654321L,
                Long.MIN_VALUE, Long.MAX_VALUE, 0xDEADBEEFL, 281474976710655L, 281474976710656L));
        Random seedGen = new Random(2024);
        for (int i = 0; i < 28; i++) {
            seeds.add(seedGen.nextLong());
        }
        int[] bounds = { 1, 2, 3, 5, 6, 7, 10, 100, 1000, 1024, 65536, 0x40000000, 999999999,
                0x40000001, Integer.MAX_VALUE };
        for (long s : seeds) {
            Random r = new Random(s);
            for (int i = 0; i < 16; i++) {
                rndRow(out, s, "int", -1, Integer.toUnsignedLong(r.nextInt()));
            }
            for (int b : bounds) {
                rndRow(out, s, "intb", b, Integer.toUnsignedLong(r.nextInt(b)));
            }
            for (int i = 0; i < 8; i++) {
                rndRow(out, s, "long", -1, r.nextLong());
            }
            for (int i = 0; i < 8; i++) {
                rndRow(out, s, "float", -1, Integer.toUnsignedLong(Float.floatToRawIntBits(r.nextFloat())));
            }
            for (int i = 0; i < 8; i++) {
                rndRow(out, s, "double", -1, Double.doubleToRawLongBits(r.nextDouble()));
            }
            for (int i = 0; i < 16; i++) {
                rndRow(out, s, "bool", -1, r.nextBoolean() ? 1L : 0L);
            }
            // consecutive gaussians exercise the cached second value of the polar method
            for (int i = 0; i < 12; i++) {
                rndRow(out, s, "gauss", -1, Double.doubleToRawLongBits(r.nextGaussian()));
            }
            // and the generator must keep working after the cache is consumed
            for (int i = 0; i < 4; i++) {
                rndRow(out, s, "int", -1, Integer.toUnsignedLong(r.nextInt()));
            }
            // setSeed discards a cached gaussian
            r.nextGaussian();
            rndRow(out, s, "gaussskip", -1, 0L);
            r.setSeed(s ^ 0x5DEECE66DL);
            rndRow(out, s ^ 0x5DEECE66DL, "reseed", -1, 0L);
            for (int i = 0; i < 4; i++) {
                rndRow(out, s ^ 0x5DEECE66DL, "gauss", -1, Double.doubleToRawLongBits(r.nextGaussian()));
            }
        }
        Files.writeString(dir.resolve("java_random.csv"), out.toString());
    }

    private static void rndRow(StringBuilder out, long seed, String op, int arg, long bits) {
        out.append(seed).append(',').append(op).append(',').append(arg).append(',')
                .append(Long.toUnsignedString(bits)).append('\n');
    }

    // ------------------------------------------------------------------------------------------
    // Input generators shared by the transcendental vectors
    // ------------------------------------------------------------------------------------------

    private static double nextUpN(double v, int n) {
        for (int i = 0; i < n; i++) {
            v = Math.nextUp(v);
        }
        return v;
    }

    private static double nextDownN(double v, int n) {
        for (int i = 0; i < n; i++) {
            v = Math.nextDown(v);
        }
        return v;
    }

    private static void addNeighbours(java.util.List<Double> out, double centre, int ulps) {
        double up = centre, down = centre;
        out.add(centre);
        for (int i = 0; i < ulps; i++) {
            up = Math.nextUp(up);
            down = Math.nextDown(down);
            out.add(up);
            out.add(down);
        }
    }

    private static double randomBits(Random r) {
        return Double.longBitsToDouble(r.nextLong());
    }

    /** A finite double whose binary exponent is uniformly drawn from [loExp, hiExp]. */
    private static double randomScaled(Random r, int loExp, int hiExp) {
        double mant = 1.0 + r.nextDouble();
        int e = loExp + r.nextInt(hiExp - loExp + 1);
        double v = Math.scalb(mant, e);
        return r.nextBoolean() ? v : -v;
    }

    private static String u(double d) {
        return Long.toUnsignedString(Double.doubleToRawLongBits(d));
    }

    // ------------------------------------------------------------------------------------------
    // fdlibm: StrictMath.acos / atan / atan2 / log
    // ------------------------------------------------------------------------------------------

    private static void writeFdlibm(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-numerics/testdata");
        Files.createDirectories(dir);
        StringBuilder out = new StringBuilder("fn,a_bits,b_bits,result_bits\n");
        Random r = new Random(0xFD11B);
        int[] mismatch = new int[3];

        // ---- acos
        java.util.ArrayList<Double> in = new java.util.ArrayList<>();
        for (double c : new double[] { 0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0,
                Math.scalb(1.0, -57), -Math.scalb(1.0, -57), Math.scalb(1.0, -58),
                Double.MIN_VALUE, -Double.MIN_VALUE, Double.MIN_NORMAL, Double.MAX_VALUE,
                Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY,
                0.25, -0.25, 0.75, -0.75, Math.cos(0.13962634), Math.cos(Math.PI / 4) }) {
            addNeighbours(in, c, c == 0.5 || c == -0.5 || c == 1.0 || c == -1.0 ? 24 : 6);
        }
        for (int i = 0; i < 1200; i++) {
            in.add(r.nextDouble() * 2 - 1);
        }
        for (int i = 0; i < 250; i++) {
            in.add(1.0 - r.nextDouble() * 1e-3);
            in.add(-1.0 + r.nextDouble() * 1e-3);
        }
        for (int i = 0; i < 150; i++) {
            in.add(1.0 - Math.scalb(r.nextDouble(), -r.nextInt(53)));
            in.add(-(1.0 - Math.scalb(r.nextDouble(), -r.nextInt(53))));
        }
        for (int i = 0; i < 500; i++) {
            in.add(randomScaled(r, -70, -1));
        }
        for (int i = 0; i < 600; i++) {
            in.add(Math.cos(r.nextDouble() * Math.PI));
        }
        for (int i = 0; i < 300; i++) {
            in.add(randomBits(r));
        }
        for (double x : in) {
            double want = StrictMath.acos(x);
            if (Double.doubleToRawLongBits(Math.acos(x)) != Double.doubleToRawLongBits(want) && !Double.isNaN(want)) {
                mismatch[0]++;
            }
            out.append("acos,").append(u(x)).append(",0,").append(u(want)).append('\n');
        }

        // ---- atan
        in.clear();
        double[] thresholds = { Math.scalb(1.0, -29), 0.4375, 0.6875, 1.1875, 2.4375, Math.scalb(1.0, 66),
                Math.scalb(1.0, -1022), 1.0, 1.5, 0.5, 2.0, Double.MIN_VALUE, Double.MAX_VALUE,
                Double.POSITIVE_INFINITY, Double.NaN, 0.0 };
        for (double c : thresholds) {
            addNeighbours(in, c, 10);
            addNeighbours(in, -c, 3);
        }
        for (int i = 0; i < 1500; i++) {
            in.add(randomScaled(r, -40, 80));
        }
        for (int i = 0; i < 600; i++) {
            in.add((r.nextDouble() * 2 - 1) * 4);
        }
        for (int i = 0; i < 400; i++) {
            in.add(randomBits(r));
        }
        for (double x : in) {
            double want = StrictMath.atan(x);
            if (Double.doubleToRawLongBits(Math.atan(x)) != Double.doubleToRawLongBits(want) && !Double.isNaN(want)) {
                mismatch[1]++;
            }
            out.append("atan,").append(u(x)).append(",0,").append(u(want)).append('\n');
        }

        // ---- atan2(y, x)
        double[] special = { 0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5, Double.MIN_VALUE, -Double.MIN_VALUE,
                1e-300, -1e-300, 1e300, -1e300, Double.MAX_VALUE, -Double.MAX_VALUE,
                Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY, Double.NaN, Math.nextUp(1.0), Math.nextDown(1.0) };
        java.util.ArrayList<double[]> pairs = new java.util.ArrayList<>();
        for (double y : special) {
            for (double x : special) {
                pairs.add(new double[] { y, x });
            }
        }
        for (int i = 0; i < 1200; i++) {
            pairs.add(new double[] { randomScaled(r, -12, 12), randomScaled(r, -12, 12) });
        }
        for (int i = 0; i < 1200; i++) {
            double a = r.nextDouble() * 2 * Math.PI, m = randomScaled(r, -6, 6);
            pairs.add(new double[] { m * Math.sin(a), m * Math.cos(a) });
        }
        for (int i = 0; i < 200; i++) {
            double x = randomScaled(r, -300, 300);
            double y = x * Math.scalb(1.0 + r.nextDouble(), (r.nextBoolean() ? 1 : -1) * (56 + r.nextInt(10)));
            pairs.add(new double[] { y, x });
        }
        for (int i = 0; i < 120; i++) {
            double x = randomScaled(r, -20, 20);
            pairs.add(new double[] { Math.nextUp(x), x });
            pairs.add(new double[] { x, -x });
            pairs.add(new double[] { 0.0, x });
            pairs.add(new double[] { -0.0, x });
            pairs.add(new double[] { x, 0.0 });
            pairs.add(new double[] { x, 1.0 });
        }
        for (int i = 0; i < 400; i++) {
            pairs.add(new double[] { randomBits(r), randomBits(r) });
        }
        for (double[] p : pairs) {
            double want = StrictMath.atan2(p[0], p[1]);
            if (Double.doubleToRawLongBits(Math.atan2(p[0], p[1])) != Double.doubleToRawLongBits(want) && !Double.isNaN(want)) {
                mismatch[2]++;
            }
            out.append("atan2,").append(u(p[0])).append(',').append(u(p[1])).append(',').append(u(want)).append('\n');
        }

        // ---- log
        in.clear();
        for (double c : new double[] { 0.0, -0.0, 1.0, -1.0, 2.0, 0.5, 10.0, Math.E, Double.MIN_VALUE,
                Double.MIN_NORMAL, Double.MAX_VALUE, Double.NaN, Double.POSITIVE_INFINITY,
                Double.NEGATIVE_INFINITY, 0.5, 0.7071067811865476, 1.4142135623730951 }) {
            addNeighbours(in, c, 12);
        }
        for (int e = -1074; e <= 1023; e += 7) {
            in.add(Math.scalb(1.0, Math.max(e, -1074)));
        }
        for (int i = 0; i < 1500; i++) {
            in.add(r.nextDouble());
        }
        for (int i = 0; i < 700; i++) {
            in.add(1.0 + (r.nextDouble() - 0.5) * Math.scalb(1.0, -19 + r.nextInt(18)));
        }
        for (int i = 0; i < 1000; i++) {
            double v1 = 2 * r.nextDouble() - 1, v2 = 2 * r.nextDouble() - 1;
            double s = v1 * v1 + v2 * v2;
            if (s < 1 && s != 0) {
                in.add(s);
            }
        }
        for (int i = 0; i < 600; i++) {
            in.add(Math.abs(randomScaled(r, -1022, 1023)));
        }
        for (int i = 0; i < 400; i++) {
            in.add(Double.longBitsToDouble(r.nextLong() & 0x000fffffffffffffL)); // subnormals
        }
        for (int i = 0; i < 400; i++) {
            in.add(randomBits(r));
        }
        for (double x : in) {
            out.append("log,").append(u(x)).append(",0,").append(u(StrictMath.log(x))).append('\n');
        }
        if (mismatch[0] + mismatch[1] + mismatch[2] != 0) {
            throw new IllegalStateException("Math.{acos,atan,atan2} differ from StrictMath on this JVM: "
                    + java.util.Arrays.toString(mismatch));
        }
        Files.writeString(dir.resolve("fdlibm_reference.csv"), out.toString());
    }

    // ------------------------------------------------------------------------------------------
    // Math.log as HotSpot runs it, next to the correctly rounded value
    // ------------------------------------------------------------------------------------------

    private static final MathContext MC = new MathContext(80);
    private static final BigDecimal LN2 = atanhSeries(BigDecimal.ONE.divide(BigDecimal.valueOf(3), MC))
            .multiply(BigDecimal.valueOf(2), MC);

    private static BigDecimal atanhSeries(BigDecimal z) {
        BigDecimal z2 = z.multiply(z, MC);
        BigDecimal term = z;
        BigDecimal sum = BigDecimal.ZERO;
        BigDecimal eps = new BigDecimal("1e-85");
        for (int k = 0; k < 400; k++) {
            sum = sum.add(term.divide(BigDecimal.valueOf(2L * k + 1), MC), MC);
            term = term.multiply(z2, MC);
            if (term.abs().compareTo(eps) < 0) {
                break;
            }
        }
        return sum;
    }

    /** The correctly rounded natural logarithm of a positive finite double. */
    private static double correctlyRoundedLog(double x) {
        int shift = 0;
        if (x < Double.MIN_NORMAL) {
            x = Math.scalb(x, 54);
            shift = -54;
        }
        int e = Math.getExponent(x);
        double m = Math.scalb(x, -e);
        if (m > 1.4142135623730951) {
            m /= 2;
            e++;
        }
        BigDecimal bm = new BigDecimal(m);
        BigDecimal z = bm.subtract(BigDecimal.ONE).divide(bm.add(BigDecimal.ONE), MC);
        BigDecimal lnm = atanhSeries(z).multiply(BigDecimal.valueOf(2), MC);
        return lnm.add(LN2.multiply(BigDecimal.valueOf(e + shift), MC), MC).doubleValue();
    }

    private static void writeHotspotLog(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-numerics/testdata");
        Files.createDirectories(dir);
        StringBuilder out = new StringBuilder("a_bits,math_log_bits,correctly_rounded_bits\n");
        Random r = new Random(0x10C);
        java.util.ArrayList<Double> in = new java.util.ArrayList<>();
        for (double c : new double[] { 1.0, 2.0, 0.5, 10.0, Math.E, Double.MIN_VALUE, Double.MIN_NORMAL,
                Double.MAX_VALUE, 0.7071067811865476, 1.4142135623730951 }) {
            addNeighbours(in, c, 16);
        }
        for (int e = -1074; e <= 1023; e += 5) {
            double p = Math.scalb(1.0, Math.max(e, -1074));
            in.add(p);
            in.add(Math.nextUp(p));
            in.add(Math.nextDown(p));
        }
        for (int i = 0; i < 5000; i++) {
            in.add(r.nextDouble());
        }
        for (int i = 0; i < 1000; i++) {
            in.add(1.0 + (r.nextDouble() - 0.5) * Math.scalb(1.0, -2 - r.nextInt(50)));
        }
        for (int i = 0; i < 2500; i++) {
            double v1 = 2 * r.nextDouble() - 1, v2 = 2 * r.nextDouble() - 1;
            double s = v1 * v1 + v2 * v2;
            if (s < 1 && s != 0) {
                in.add(s);
            }
        }
        for (int i = 0; i < 1000; i++) {
            in.add(Math.abs(randomScaled(r, -1022, 1023)));
        }
        for (int i = 0; i < 500; i++) {
            double v = Double.longBitsToDouble(r.nextLong() & 0x000fffffffffffffL);
            if (v != 0) {
                in.add(v);
            }
        }
        for (double x : in) {
            if (x <= 0 || Double.isNaN(x) || Double.isInfinite(x) || x == 1.0) {
                continue;
            }
            out.append(u(x)).append(',').append(u(Math.log(x))).append(',')
                    .append(u(correctlyRoundedLog(x))).append('\n');
        }
        Files.writeString(dir.resolve("hotspot_log.csv"), out.toString());
    }
}
