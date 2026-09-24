package workbench;

import com.fasterxml.jackson.databind.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;

/** Engine adapters accept only a bounded single request. OS isolation is mandatory. */
final class Protocol {
    static final ObjectMapper JSON = new ObjectMapper().enable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES).enable(DeserializationFeature.FAIL_ON_TRAILING_TOKENS).enable(com.fasterxml.jackson.core.JsonParser.Feature.STRICT_DUPLICATE_DETECTION);
    /** Fixed probe hints only; absent property leaves normal adapters unchanged. */
    static void checkpoint(String value) throws IOException {
        if (!Boolean.getBoolean("workbench.probe")) return;
        if (!Set.of("file_worker_entered", "metadata_read", "request_decoded",
                "parser_selected", "search_selected", "worker_returned",
                "pdf_load_started", "pdf_loaded", "pdf_stripper_started",
                "pdf_stripper_ready", "pdf_text_started", "pdf_text_finished").contains(value))
            throw new IOException("Invalid fixed checkpoint");
        Files.writeString(Path.of("java-checkpoint.json"), "\"" + value + "\"");
    }
    static JsonNode read() throws IOException {
        byte[] bytes = System.in.readNBytes(1_048_577);
        if (bytes.length > 1_048_576) throw new IOException("Request too large");
        JsonNode request = JSON.readTree(bytes);
        if (request == null || request.path("protocol_version").asInt() != 1) throw new IOException("Unsupported protocol");
        UUID.fromString(request.path("job_id").asText());
        return request;
    }
    static Path input(String value) throws IOException {
        Path path = relative(value);
        // Fixed Windows file-IPC recipes assign exactly one immutable input.
        // The property is a coordinator argument, never part of worker JSON.
        // With no property, the existing macOS relative-path contract is unchanged.
        String assigned = System.getProperty("workbench.assignedInput");
        if (assigned != null) {
            if (!value.equals("input.json")) throw new IOException("Unassigned input name");
            path = Path.of(assigned);
            if (!path.isAbsolute()) throw new IOException("Assigned input must be absolute");
        }
        if (!Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS) || Files.isSymbolicLink(path) || Files.size(path) > 16 * 1024 * 1024) throw new IOException("Invalid input file");
        return path;
    }
    static Path relative(String value) throws IOException {
        Path path = Path.of(value);
        if (value.isEmpty() || value.contains("\\") || value.contains(":") || path.isAbsolute() || value.startsWith(".")) throw new IOException("Unsafe path");
        for (Path component : path) if (component.toString().equals("..") || Files.isSymbolicLink(component)) throw new IOException("Unsafe path");
        Path root = Path.of("").toAbsolutePath().normalize();
        Path resolved = root.resolve(path).normalize();
        if (!resolved.startsWith(root)) throw new IOException("Path escape");
        Path cursor=root;
        for(Path component:path){cursor=cursor.resolve(component);if(Files.isSymbolicLink(cursor))throw new IOException("Symlink input");}
        return path;
    }
    static void write(JsonNode request, Object result) throws IOException {
        Path output=relative(request.path("output").asText());
        byte[] bytes=JSON.writeValueAsBytes(result);
        long limit=request.path("limits").path("output_bytes").asLong(0);
        if(limit<=0 || limit>64*1024*1024 || bytes.length>limit)throw new IOException("Output limit exceeded");
        Files.write(output,bytes,StandardOpenOption.CREATE_NEW);
        System.out.println(JSON.writeValueAsString(Map.of("protocol_version",1,"job_id",request.path("job_id").asText(),"output",output.toString(),"bytes",bytes.length)));
    }
    static void failure(Exception exception) {
        if (Boolean.getBoolean("workbench.debug")) exception.printStackTrace(System.err);
        // Do not leak local paths, document content or stack traces over IPC.
        System.out.println("{\"protocol_version\":1,\"state\":\"failed\",\"error\":\"Engine operation failed\"}");
        System.exit(1);
    }
}
