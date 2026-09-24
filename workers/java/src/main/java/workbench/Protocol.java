package workbench;

import com.fasterxml.jackson.databind.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.lang.management.ManagementFactory;

/** Engine adapters accept only a bounded single request. OS isolation is mandatory. */
final class Protocol {
    static final ObjectMapper JSON = new ObjectMapper().enable(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES).enable(DeserializationFeature.FAIL_ON_TRAILING_TOKENS).enable(com.fasterxml.jackson.core.JsonParser.Feature.STRICT_DUPLICATE_DETECTION);
    /** Fixed probe hints only; absent property leaves normal adapters unchanged. */
    static void checkpoint(String value) throws IOException {
        if (!Boolean.getBoolean("workbench.probe")) return;
        if (!Set.of("file_worker_entered", "metadata_read", "request_decoded",
                "parser_selected", "search_selected", "worker_returned",
                "pdf_load_started", "pdf_loaded", "pdf_stripper_started",
                "pdf_stripper_ready", "pdf_text_started", "pdf_text_finished",
                "search_request_validated", "search_index_validated", "search_directory_started",
                "search_directory_ready", "search_manifest_read", "search_writer_started",
                "search_writer_ready", "search_index_committed",
                "search_realpath_started", "search_realpath_ready").contains(value))
            throw new IOException("Invalid fixed checkpoint");
        Files.writeString(Path.of("java-checkpoint.json"), "\"" + value + "\"");
    }
    /** One bounded observation of this invocation's main thread, only in probes. */
    static void startSample() {
        if (!Boolean.getBoolean("workbench.probe")) return;
        long mainThread=Thread.currentThread().threadId();
        Thread sampler=new Thread(()-> {
            try {
                Thread.sleep(20_000);
                var info=ManagementFactory.getThreadMXBean().getThreadInfo(mainThread,64);
                if(info==null)return;
                byte[] bytes=sampleBytes(info.getThreadState(),info.getStackTrace());
                Files.write(Path.of("java-sample.json"),bytes,StandardOpenOption.CREATE_NEW);
            } catch(Exception unavailable) { /* missing is unknown; never publish raw diagnostics */ }
        },"workbench-probe-sampler");
        try {
            sampler.setDaemon(true);
            sampler.start();
        } catch(RuntimeException unavailable) { /* keep original operation */ }
    }
    static byte[] sampleBytes(Thread.State state,StackTraceElement[] frames)throws IOException {
        if(frames.length>64)throw new IOException("Sample frame bound exceeded");
        Map<String,Object> result=new LinkedHashMap<>();
        result.put("state",state.name());result.put("frame_count",frames.length);
        for(String category:List.of("font_provider","font_directory_walk","font_decode","pdf_text","class_loading","file_io"))result.put(category,false);
        for(StackTraceElement frame:frames) {
            // Never read/serialize method names, source files, line numbers,
            // classloader/module strings, thread names/IDs or exception text.
            String type=frame.getClassName();
            String category=switch(type) {
                case "org.apache.pdfbox.pdmodel.font.FileSystemFontProvider" -> "font_provider";
                case "org.apache.fontbox.util.autodetect.FontFileFinder",
                     "org.apache.fontbox.util.autodetect.WindowsFontDirFinder" -> "font_directory_walk";
                case "org.apache.fontbox.ttf.FontHeaders", "org.apache.fontbox.ttf.TTFParser",
                     "org.apache.fontbox.ttf.OTFParser", "org.apache.fontbox.ttf.OpenTypeFont",
                     "org.apache.fontbox.ttf.TrueTypeFont", "org.apache.fontbox.ttf.TrueTypeCollection",
                     "org.apache.fontbox.type1.Type1Font" -> "font_decode";
                case "org.apache.pdfbox.text.PDFTextStripper", "org.apache.pdfbox.contentstream.PDFStreamEngine",
                     "org.apache.pdfbox.text.LegacyPDFStreamEngine" -> "pdf_text";
                case "java.lang.ClassLoader", "jdk.internal.loader.BuiltinClassLoader" -> "class_loading";
                case "java.io.FileInputStream", "java.io.RandomAccessFile",
                     "sun.nio.ch.FileDispatcherImpl", "sun.nio.ch.FileChannelImpl" -> "file_io";
                default -> null;
            };
            if(category!=null)result.put(category,true);
        }
        byte[] bytes=JSON.writeValueAsBytes(result);
        if(bytes.length>512)throw new IOException("Sample output bound exceeded");
        return bytes;
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
    /** Closed diagnostic vocabulary: never inspect messages, causes or stack traces. */
    static byte[] failureBytes(Exception exception) {
        Class<?> type=exception.getClass();
        String category=type==AccessDeniedException.class ? "access_denied"
            : type==NoSuchFileException.class ? "missing_file"
            : type==FileAlreadyExistsException.class ? "file_exists"
            : type==FileSystemException.class ? "filesystem"
            : type==IOException.class ? "io"
            : type==SecurityException.class ? "security" : "other";
        return ("{\"category\":\""+category+"\"}").getBytes(java.nio.charset.StandardCharsets.UTF_8);
    }
    static void failure(Exception exception) {
        try {
            if(Boolean.getBoolean("workbench.probe"))
                Files.write(Path.of("java-failure.json"),failureBytes(exception),StandardOpenOption.CREATE_NEW);
        } catch(Exception unavailable) { /* diagnostic failure never replaces original exit */ }

        if (Boolean.getBoolean("workbench.debug")) exception.printStackTrace(System.err);
        // Do not leak local paths, document content or stack traces over IPC.
        System.out.println("{\"protocol_version\":1,\"state\":\"failed\",\"error\":\"Engine operation failed\"}");
        System.exit(1);
    }
}
