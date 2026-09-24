package workbench;

import java.nio.file.*;
import java.net.*;
import java.util.*;

/** Deliberately hostile fixture. Arguments must point only at disposable sentinels. */
public final class HostileProbe {
    @FunctionalInterface private interface Attempt { void run() throws Exception; }
    private static boolean allowed(Attempt attempt) {
        try { attempt.run(); return true; } catch (Exception exception) { return false; }
    }
    public static void main(String[] args) throws Exception {
        String mode=args[0];
        if(mode.equals("timeout")) { Thread.sleep(60_000); return; }
        if(mode.equals("oversize")) {
            try(var output=Files.newOutputStream(Path.of("scratch/oversize.bin"))) {
                byte[] block=new byte[1_048_576];
                for(int i=0;i<65;i++)output.write(block);
            }
            return;
        }
        Map<String,Boolean> result=new LinkedHashMap<>();
        result.put("data_volume_alias_read",allowed(()->Files.readString(Path.of("/System/Volumes/Data"+args[1]))));
        result.put("other_workspace_read",allowed(()->Files.readString(Path.of(args[1]))));
        result.put("original_write",allowed(()->Files.writeString(Path.of(args[2]),"modified")));
        result.put("direct_network",allowed(()->{
            try(var socket=new Socket()) {socket.connect(new InetSocketAddress("127.0.0.1",Integer.parseInt(args[3])),1000);}
        }));
        result.put("input_read",allowed(()->Files.readString(Path.of("input.json"))));
        result.put("input_write",allowed(()->Files.writeString(Path.of("input.json"),"modified")));
        result.put("scratch_write",allowed(()->Files.writeString(Path.of("scratch/allowed.txt"),"permitted")));
        result.put("index_write",allowed(()->Files.writeString(Path.of(System.getProperty("workbench.index"),"probe.txt"),"modified")));
        result.put("sibling_job_read",allowed(()->Files.readString(Path.of(args[4]))));
        result.put("child_process",allowed(()->{
            Process process=new ProcessBuilder(Path.of(System.getProperty("java.home"),"bin/java").toString(),"-version").start();
            try {process.waitFor();} finally {process.destroyForcibly();}
        }));
        result.put("caller_environment",System.getenv("WORKBENCH_TEST_SECRET")!=null);
        result.put("profile_read",allowed(()->Files.readString(Path.of("worker.sb"))));
        Files.write(Path.of("result.json"),Protocol.JSON.writeValueAsBytes(result),StandardOpenOption.CREATE_NEW);
    }
}
