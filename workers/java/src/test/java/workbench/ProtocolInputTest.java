package workbench;

import java.io.IOException;
import java.nio.file.*;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import static org.junit.jupiter.api.Assertions.*;

class ProtocolInputTest {
    @TempDir Path root;
    @Test void assignedInputAllowsOnlyTheFixedNameAndOrdinaryFile() throws Exception {
        Path file=root.resolve("assigned.bin");Files.writeString(file,"synthetic");
        try {
            System.setProperty("workbench.assignedInput",file.toString());
            assertEquals(file,Protocol.input("input.json"));
            for(String value:new String[]{"another.json","../input.json","a/input.json","Q:input.json","a\\input.json"})
                assertThrows(IOException.class,()->Protocol.input(value));
            System.setProperty("workbench.assignedInput",root.toString());
            assertThrows(IOException.class,()->Protocol.input("input.json"));
            System.setProperty("workbench.assignedInput","relative.bin");
            assertThrows(IOException.class,()->Protocol.input("input.json"));
        } finally { System.clearProperty("workbench.assignedInput"); }
    }
    @Test void absentPropertyKeepsTheRelativePathBoundary() throws Exception {
        assertNull(System.getProperty("workbench.assignedInput"));
        for(String value:new String[]{"../input.json","Q:input.json","a\\input.json",root.resolve("assigned.bin").toString()})
            assertThrows(IOException.class,()->Protocol.input(value));
    }
}
