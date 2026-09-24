package workbench;

import java.net.*;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.channels.SeekableByteChannel;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.TimeUnit;

/** Reviewed synthetic fixture, included only by the development staging script. */
public final class WindowsJavaProbe {
    @FunctionalInterface private interface Attempt { void run() throws Exception; }
    private static boolean allowed(Attempt operation) {
        try { operation.run(); return true; } catch (Exception denied) { return false; }
    }
    private static boolean opened(Path path,StandardOpenOption access) throws Exception {
        SeekableByteChannel channel;
        try { channel=Files.newByteChannel(path,access); }
        catch(IOException denied) { return false; }
        // A later read/close failure is a failed probe, never reclassified as
        // denied access after a handle with the requested right was obtained.
        try(channel) { if(access==StandardOpenOption.READ)channel.read(ByteBuffer.allocate(1)); }
        return true;
    }
    private static boolean connected(int port) throws Exception {
        try(var socket=new Socket()) {
            try { socket.connect(new InetSocketAddress("127.0.0.1",port),1000); }
            catch(IOException denied) { return false; }
            return true;
        }
    }
    private static boolean childCreated() throws Exception {
        Process child;
        try {
            child=new ProcessBuilder(Path.of(System.getProperty("java.home"),"bin/java.exe").toString(),"--version")
                .redirectOutput(ProcessBuilder.Redirect.DISCARD).redirectError(ProcessBuilder.Redirect.DISCARD).start();
        } catch(IOException denied) { return false; }
        try { child.destroyForcibly(); if(!child.waitFor(5,TimeUnit.SECONDS))throw new IllegalStateException("Synthetic child did not terminate"); }
        finally { child.destroyForcibly(); }
        return true;
    }
    private static boolean available(String type) {
        try { Class.forName(type);return true; } catch (ClassNotFoundException absent) { return false; }
    }
    public static void main(String[] args) throws Exception {
        if(args.length!=7 || !(args[0].equals("parser")||args[0].equals("search"))) throw new IllegalArgumentException("Invalid synthetic probe");
        Path input=Path.of(System.getProperty("workbench.assignedInput"));
        Path request=Path.of(System.getProperty("workbench.assignedRequest"));
        Path runtime=Path.of(args[1]);
        Map<String,Boolean> result=new TreeMap<>();
        result.put("assigned_input_read",opened(input,StandardOpenOption.READ));
        result.put("assigned_input_write",opened(input,StandardOpenOption.WRITE));
        result.put("assigned_request_read",opened(request,StandardOpenOption.READ));
        result.put("assigned_request_write",opened(request,StandardOpenOption.WRITE));
        result.put("runtime_write",opened(runtime.resolve("worker.jar"),StandardOpenOption.WRITE));
        result.put("other_runtime_read",opened(Path.of(args[2]),StandardOpenOption.READ));
        result.put("other_workspace_read",opened(Path.of(args[3]),StandardOpenOption.READ));
        result.put("original_read",opened(Path.of(args[4]),StandardOpenOption.READ));
        result.put("original_write",opened(Path.of(args[4]),StandardOpenOption.WRITE));
        result.put("sibling_index_read",opened(Path.of(args[5]),StandardOpenOption.READ));
        result.put("scratch_write",allowed(()->Files.writeString(Path.of("sentinel.txt"),"synthetic scratch")));
        result.put("direct_network",connected(Integer.parseInt(args[6])));
        result.put("child_process",childCreated());
        result.put("caller_environment",System.getenv("EW_SYNTHETIC_JAVA_SECRET")!=null);
        result.put("parser_class",available("workbench.ParseWorker"));
        result.put("search_class",available("workbench.SearchWorker"));
        result.put("tika_class",available("org.apache.tika.parser.microsoft.ooxml.OOXMLParser"));
        result.put("lucene_class",available("org.apache.lucene.index.IndexWriter"));
        String index=System.getProperty("workbench.probeIndex");
        result.put("assigned_index_read",index!=null && opened(Path.of(index),StandardOpenOption.READ));
        result.put("assigned_index_write",index!=null && opened(Path.of(index),StandardOpenOption.WRITE));
        Files.write(Path.of("result.json"),Protocol.JSON.writeValueAsBytes(result),StandardOpenOption.CREATE_NEW);
    }
}
