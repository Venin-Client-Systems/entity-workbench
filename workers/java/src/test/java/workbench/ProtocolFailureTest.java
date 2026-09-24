package workbench;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import static org.junit.jupiter.api.Assertions.*;

class ProtocolFailureTest {
    @TempDir Path root;
    @Test void knownExceptionsEmitOnlyClosedCategories() {
        Exception[] failures={new AccessDeniedException("PRIVATE/path"),new NoSuchFileException("PRIVATE/path"),
            new FileAlreadyExistsException("PRIVATE/path"),new FileSystemException("PRIVATE/path","PRIVATE/other","PRIVATE-reason"),
            new IOException("PRIVATE-message"),new SecurityException("PRIVATE-message")};
        String[] categories={"access_denied","missing_file","file_exists","filesystem","io","security"};
        for(int i=0;i<failures.length;i++) {
            byte[] bytes=Protocol.failureBytes(failures[i]);
            assertEquals("{\"category\":\""+categories[i]+"\"}",new String(bytes,StandardCharsets.UTF_8));
            assertTrue(bytes.length<=128);
        }
    }
    @Test void unknownTypesAndSubclassesDoNotExposeOrInspectMetadata() {
        class PrivateFailure extends IOException {
            @Override public String getMessage(){throw new AssertionError("must not inspect message");}
            @Override public synchronized Throwable getCause(){throw new AssertionError("must not inspect cause");}
            @Override public String toString(){throw new AssertionError("must not stringify exception");}
        }
        for(Exception failure:new Exception[]{new PrivateFailure(),new IllegalArgumentException("PRIVATE"),new Exception("PRIVATE")})
            assertEquals("{\"category\":\"other\"}",new String(Protocol.failureBytes(failure),StandardCharsets.UTF_8));
    }
    @Test void diagnosticAbsenceOrWriteFailureNeverChangesOriginalExit() throws Exception {
        String javaExecutable=Path.of(System.getProperty("java.home"),"bin",System.getProperty("os.name").startsWith("Windows")?"java.exe":"java").toString();
        String classpath=System.getProperty("surefire.test.class.path",System.getProperty("java.class.path"));
        for(String mode:new String[]{"absent","enabled","write_denied"}) {
            Path scratch=Files.createDirectory(root.resolve(mode));
            Path output=scratch.resolve("java-failure.json");
            if(mode.equals("write_denied"))Files.createDirectory(output);
            var arguments=new java.util.ArrayList<String>();arguments.add(javaExecutable);
            if(!mode.equals("absent"))arguments.add("-Dworkbench.probe=true");
            arguments.addAll(java.util.List.of("-cp",classpath,FailureChild.class.getName()));
            Process child=new ProcessBuilder(arguments).directory(scratch.toFile())
                .redirectOutput(ProcessBuilder.Redirect.DISCARD).redirectError(ProcessBuilder.Redirect.DISCARD).start();
            try {
                assertTrue(child.waitFor(10,TimeUnit.SECONDS),"synthetic failure control timed out");
                assertEquals(1,child.exitValue());
            } finally {
                if(child.isAlive()){child.destroyForcibly();assertTrue(child.waitFor(5,TimeUnit.SECONDS));}
            }
            if(mode.equals("absent"))assertFalse(Files.exists(output));
            if(mode.equals("enabled"))assertEquals("{\"category\":\"io\"}",Files.readString(output));
            if(mode.equals("write_denied"))assertTrue(Files.isDirectory(output));
        }
    }
    public static final class FailureChild {
        public static void main(String[] args){Protocol.failure(new IOException("PRIVATE-synthetic-message"));}
    }
}
