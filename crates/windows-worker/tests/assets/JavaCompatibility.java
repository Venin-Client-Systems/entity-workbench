import java.nio.file.*;
/** Synthetic Java 21 assigned-I/O compatibility probe; no case data or network. */
public final class JavaCompatibility {
    public static void main(String[] args) throws Exception {
        if (Runtime.version().feature() != 21) throw new IllegalStateException("Java 21 required");
        if (!Files.readString(Path.of(args[0])).equals("synthetic Java input")) {
            throw new IllegalStateException("assigned input mismatch");
        }
        Files.writeString(Path.of("result.json"), "{\"java_input\":true,\"java_output\":true}", StandardOpenOption.CREATE_NEW);
    }
}
