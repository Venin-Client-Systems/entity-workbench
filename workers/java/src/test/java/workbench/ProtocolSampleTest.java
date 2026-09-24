package workbench;

import java.io.IOException;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class ProtocolSampleTest {
    private StackTraceElement frame(String type) {
        return new StackTraceElement("PRIVATE-loader","PRIVATE-module","PRIVATE-version",
            type,"PRIVATE-method","PRIVATE-file",123);
    }
    @Test void knownFramesBecomeFixedCategoriesWithoutMetadata() throws Exception {
        byte[] bytes=Protocol.sampleBytes(Thread.State.RUNNABLE,new StackTraceElement[]{
            frame("org.apache.pdfbox.pdmodel.font.FileSystemFontProvider"),
            frame("org.apache.fontbox.util.autodetect.FontFileFinder"),
            frame("org.apache.fontbox.ttf.FontHeaders"),
            frame("org.apache.pdfbox.text.PDFTextStripper"),
            frame("jdk.internal.loader.BuiltinClassLoader"),frame("java.io.FileInputStream")});
        var value=Protocol.JSON.readTree(bytes);
        assertEquals(6,value.get("frame_count").asInt());
        assertEquals("RUNNABLE",value.get("state").asText());
        for(String key:new String[]{"font_provider","font_directory_walk","font_decode","pdf_text","class_loading","file_io"})
            assertTrue(value.get(key).asBoolean());
        String text=new String(bytes,java.nio.charset.StandardCharsets.UTF_8);
        assertFalse(text.contains("PRIVATE"));assertFalse(text.contains("org.apache"));
        assertTrue(bytes.length<=512);
    }
    @Test void unknownFramesAndPrefixSpoofsDoNotPublishSourceText() throws Exception {
        var value=Protocol.JSON.readTree(Protocol.sampleBytes(Thread.State.BLOCKED,new StackTraceElement[]{
            frame("org.apache.pdfbox.pdmodel.font.FileSystemFontProviderPRIVATE"),
            frame("PRIVATE-content".repeat(1024))}));
        assertEquals(2,value.get("frame_count").asInt());assertFalse(value.get("font_provider").asBoolean());
        assertFalse(value.toString().contains("PRIVATE"));
        assertEquals(8,value.size());
    }
    @Test void frameLimitIsEnforcedBeforeInspection() throws Exception {
        assertThrows(IOException.class,()->Protocol.sampleBytes(Thread.State.RUNNABLE,new StackTraceElement[65]));
        StackTraceElement[] frames=new StackTraceElement[64];
        java.util.Arrays.fill(frames,frame("PRIVATE"));
        assertTrue(Protocol.sampleBytes(Thread.State.TIMED_WAITING,frames).length<=512);
    }
}
