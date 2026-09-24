package workbench;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.io.PrintStream;
import java.nio.file.*;

/** Fixed file IPC bridge. It opens its own request; no inherited handles needed. */
public final class FileWorker {
    public static void main(String[] args) {
        // This entry point uses files only. A detached AppContainer has no
        // inherited console handles; the existing Protocol's console notices
        // are irrelevant here. Do not change the stdin-based macOS entrypoints.
        System.setOut(new PrintStream(OutputStream.nullOutputStream()));
        System.setErr(new PrintStream(OutputStream.nullOutputStream()));
        try {
            Protocol.checkpoint("file_worker_entered");
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
            Protocol.checkpoint("metadata_read");
            var request = Protocol.JSON.readTree(bytes);
            Protocol.checkpoint("request_decoded");
            if (request == null || !args[0].equals(request.path("operation").asText()))
                throw new IOException("Recipe/request mismatch");
            System.setIn(new ByteArrayInputStream(bytes));
            if (args[0].equals("parse")) {
                Protocol.checkpoint("parser_selected");
                ParseWorker.main(new String[0]);
            } else {
                Protocol.checkpoint("search_selected");
                SearchWorker.main(new String[0]);
            }
            Protocol.checkpoint("worker_returned");
        } catch (Exception error) { Protocol.failure(error); }
    }
}
