package workbench;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.nio.file.*;

/** Fixed file IPC bridge. It opens its own request; no inherited handles needed. */
public final class FileWorker {
    public static void main(String[] args) {
        try {
            if (args.length != 2 || !(args[0].equals("parse") || args[0].equals("index") || args[0].equals("search")))
                throw new IOException("Unsupported fixed recipe");
            Path path = Path.of(args[1]);
            if (!path.isAbsolute() || !Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS) || Files.size(path) > 1_048_576)
                throw new IOException("Invalid assigned request");
            byte[] bytes;
            try (var input = Files.newInputStream(path, LinkOption.NOFOLLOW_LINKS)) {
                bytes = input.readNBytes(1_048_577);
            }
            if (bytes.length > 1_048_576) throw new IOException("Request grew beyond bound");
            var request = Protocol.JSON.readTree(bytes);
            if (request == null || !args[0].equals(request.path("operation").asText()))
                throw new IOException("Recipe/request mismatch");
            System.setIn(new ByteArrayInputStream(bytes));
            if (args[0].equals("parse")) ParseWorker.main(new String[0]);
            else SearchWorker.main(new String[0]);
        } catch (Exception error) { Protocol.failure(error); }
    }
}
