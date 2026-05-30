import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Random;

// Generates reference numeric data for the Rust crates directly from the JVM, so the ports
// can be checked for bit-for-bit equality. Usage: java RefGen <repo-root>.
public final class RefGen {
    private static final float[] SIN = new float[65536];

    static {
        for (int i = 0; i < SIN.length; i++) {
            SIN[i] = (float) Math.sin((double) i * Math.PI * 2.0 / 65536.0);
        }
    }

    private static float sin(float v) {
        return SIN[(int) (v * 10430.378F) & 0xFFFF];
    }

    private static float cos(float v) {
        return SIN[(int) (v * 10430.378F + 16384.0F) & 0xFFFF];
    }

    public static void main(String[] args) throws IOException {
        Path root = Paths.get(args.length > 0 ? args[0] : ".");
        writeSinTable(root);
        writeMthSamples(root);
        writeJavaRandom(root);
        System.out.println("reference data written under " + root.toAbsolutePath());
    }

    private static void writeSinTable(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-numerics/data");
        Files.createDirectories(dir);
        ByteBuffer buf = ByteBuffer.allocate(SIN.length * 4).order(ByteOrder.LITTLE_ENDIAN);
        for (float v : SIN) {
            buf.putInt(Float.floatToRawIntBits(v));
        }
        Files.write(dir.resolve("mth_sin_table.bin"), buf.array());
    }

    private static void writeMthSamples(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-numerics/testdata");
        Files.createDirectories(dir);
        StringBuilder out = new StringBuilder("x_bits,sin_bits,cos_bits\n");
        for (int i = 0; i < 128; i++) {
            mthRow(out, (i - 64) * 0.3926991F);
        }
        for (float x : new float[] {
                0F, 1F, -1F, 0.5F, 0.017453292F,
                (float) Math.PI, -(float) Math.PI, (float) (Math.PI / 2.0),
                (float) (2.0 * Math.PI), 90F, 180F, -90F, 1000F, -1000F }) {
            mthRow(out, x);
        }
        Files.writeString(dir.resolve("mth_reference.csv"), out.toString());
    }

    private static void mthRow(StringBuilder out, float x) {
        out.append(Integer.toUnsignedString(Float.floatToRawIntBits(x))).append(',')
                .append(Integer.toUnsignedString(Float.floatToRawIntBits(sin(x)))).append(',')
                .append(Integer.toUnsignedString(Float.floatToRawIntBits(cos(x)))).append('\n');
    }

    private static void writeJavaRandom(Path root) throws IOException {
        Path dir = root.resolve("crates/ms-rng/testdata");
        Files.createDirectories(dir);
        StringBuilder out = new StringBuilder("seed,op,arg,bits\n");
        long[] seeds = { 0L, 1L, -1L, 42L, 25214903917L, 123456789L, -987654321L,
                Long.MIN_VALUE, Long.MAX_VALUE };
        int[] bounds = { 2, 3, 5, 6, 7, 10, 100, 1000, 1024, 65536, 0x40000000, 999999999 };
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
        }
        Files.writeString(dir.resolve("java_random.csv"), out.toString());
    }

    private static void rndRow(StringBuilder out, long seed, String op, int arg, long bits) {
        out.append(seed).append(',').append(op).append(',').append(arg).append(',')
                .append(Long.toUnsignedString(bits)).append('\n');
    }
}
