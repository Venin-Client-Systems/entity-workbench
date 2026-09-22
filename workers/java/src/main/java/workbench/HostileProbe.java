package workbench;
import java.nio.file.*;
import java.net.*;
import java.util.*;
/** Deliberately hostile fixture. Its arguments must point only at disposable sentinels. */
public final class HostileProbe {
    public static void main(String[] args)throws Exception{
        Map<String,Boolean> result=new LinkedHashMap<>();
        try{Files.readString(Path.of(args[0]));result.put("other_workspace_read",true);}catch(Exception e){result.put("other_workspace_read",false);}
        try{Files.writeString(Path.of(args[1]),"modified");result.put("original_write",true);}catch(Exception e){result.put("original_write",false);}
        try(var socket=new Socket()){socket.connect(new InetSocketAddress("127.0.0.1",Integer.parseInt(args[2])),1000);result.put("direct_network",true);}catch(Exception e){result.put("direct_network",false);}
        try{Files.readString(Path.of("input.txt"));Files.writeString(Path.of("output.txt"),"permitted");result.put("job_io",true);}catch(Exception e){result.put("job_io",false);}
        System.out.println(Protocol.JSON.writeValueAsString(result));
    }
}
