package workbench;

import java.io.*;
import java.nio.file.*;
import java.util.*;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.apache.pdfbox.pdmodel.font.FontMappers;
import static org.junit.jupiter.api.Assertions.*;

class AppLocalFontsTest {
    @TempDir Path root;

    private Map<String,Object> extract(byte[] bytes)throws Exception {
        Path input=root.resolve("synthetic.pdf");Files.write(input,bytes);
        Map<String,Object> result=new LinkedHashMap<>();
        result.put("limitations",new ArrayList<String>());
        System.setProperty("workbench.fontPolicy",AppLocalFonts.POLICY);
        try {
            ParseWorker.pdf(input,Protocol.JSON.readTree("{\"limits\":{\"pages\":100}}"),result);
            return result;
        } finally {System.clearProperty("workbench.fontPolicy");}
    }

    @Test void standardFontsEncodingsUnicodeAndMissingCoverageStayReviewable()throws Exception {
        var result=extract(FontFixtures.corpus());String text=(String)result.get("text");
        assertEquals(AppLocalFonts.PARSER,result.get("parser"));assertEquals("partial",result.get("status"));
        for(var face:org.apache.pdfbox.pdmodel.font.Standard14Fonts.FontName.values()) {
            String expected=switch(face) {
                case SYMBOL -> "\u0391\u0392";
                case ZAPF_DINGBATS -> "\u2701\u2702";
                default -> "Standard14 caf\u00e9 123 \u20ac "+face.name();
            };
            assertEquals(expected,section(text,"STD_"+face.name()));
        }
        assertEquals("Embedded \u00e9 \u03a9 \u0416",section(text,"EMBEDDED"));
        assertEquals("Unknown explicit widths",section(text,"UNKNOWN_WIDTHS"));
        assertEquals("Unknown missing widths",section(text,"UNKNOWN_NO_WIDTHS"));
        assertEquals("\u00e9fi\u0393",section(text,"DIFFERENCES"));
        assertEquals("\u4e00\ud83d\ude00\u0627",section(text,"SIMPLE_UNICODE"));
        assertEquals("\u4e00\ud83d\ude00\u0627",section(text,"CID_UNICODE"));
        assertEquals("",section(text,"CID_UNMAPPED"));
        assertEquals(List.of("21"),((Map<?,?>)result.get("metadata")).get("pdf:pages"));
        var limits=(List<?>)result.get("limitations");
        assertTrue(limits.contains("font_substituted"));assertTrue(limits.contains("font_coverage_unverified"));
        assertFalse(Files.exists(root.resolve(".pdfbox.cache")));
    }

    private static String section(String text,String label) {
        String begin="BEGIN_"+label,end="END_"+label;
        List<String> lines=text.lines().toList();
        assertEquals(1,Collections.frequency(lines,begin));assertEquals(1,Collections.frequency(lines,end));
        int start=lines.indexOf(begin),stop=lines.indexOf(end);
        assertTrue(stop>start);return String.join("\n",lines.subList(start+1,stop)).strip();
    }

    @Test void duplicateClasspathCandidatesRejectBeforeOpeningAny()throws Exception {
        var candidate=new java.net.URI("file:/synthetic-must-not-open").toURL();
        assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installCandidates(Collections.emptyEnumeration()));
        Enumeration<java.net.URL> duplicate=new Enumeration<>() {
            int read;
            public boolean hasMoreElements(){return true;}
            public java.net.URL nextElement(){if(++read>1)throw new AssertionError("Must stop at second candidate");return candidate;}
        };
        assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installCandidates(duplicate));
        byte[] bytes;
        try(var stream=AppLocalFontsTest.class.getResourceAsStream(AppLocalFonts.RESOURCE)){bytes=stream.readAllBytes();}
        Path first=root.resolve("first"),second=root.resolve("second");
        for(Path directory:List.of(first,second)) {
            Path resource=directory.resolve(AppLocalFonts.RESOURCE.substring(1));
            Files.createDirectories(resource.getParent());Files.write(resource,bytes);
        }
        try(var loader=new java.net.URLClassLoader(new java.net.URL[]{first.toUri().toURL(),second.toUri().toURL()},null)) {
            assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installCandidates(loader.getResources(AppLocalFonts.RESOURCE.substring(1))));
        }
    }

    @Test void drawingOnlyPdfProducesExactlyOnePlatformPageSeparator()throws Exception {
        var result=extract(FontFixtures.drawingOnly());
        assertEquals("partial",result.get("status"));assertEquals(AppLocalFonts.PARSER,result.get("parser"));
        assertEquals(System.lineSeparator(),result.get("text"));
        assertEquals(List.of("1"),((Map<?,?>)result.get("metadata")).get("pdf:pages"));
        assertEquals(List.of("ocr_not_performed","embedded_documents_excluded"),result.get("limitations"));
    }

    @Test void embeddedFontDoesNotClaimFallback()throws Exception {
        var result=extract(FontFixtures.embedded());
        assertEquals("Embedded \u00e9 \u03a9 \u0416",((String)result.get("text")).strip());
        assertFalse(((List<?>)result.get("limitations")).contains("font_substituted"));
        assertFalse(((List<?>)result.get("limitations")).contains("font_coverage_unverified"));
    }

    @Test void allMapperContractsUseOneVerifiedFallbackWithoutNamePaths()throws Exception {
        try(var fonts=AppLocalFonts.install()) {
            assertSame(fonts,FontMappers.instance());assertFalse(fonts.used());
            var ttf=fonts.getTrueTypeFont("../../outside",null);var simple=fonts.getFontBoxFont("C:\\private\\font",null);
            var cid=fonts.getCIDFont("https://invalid/font",null,null);
            assertTrue(ttf.isFallback());assertTrue(simple.isFallback());assertTrue(cid.isFallback());
            assertSame(ttf.getFont(),simple.getFont());assertSame(ttf.getFont(),cid.getTrueTypeFont());
            assertTrue(fonts.used());
            assertNotEquals(0,ttf.getFont().getUnicodeCmapLookup().getGlyphId(0x00e9));
            for(int unsupported:new int[]{0x0627,0x4e00,0x2701})assertEquals(0,ttf.getFont().getUnicodeCmapLookup().getGlyphId(unsupported));
        }
    }

    @Test void malformedMissingChangedAndOversizedAssetsRejectBeforeInstallation()throws Exception {
        assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installResource(null));
        for(int size:new int[]{0,20,AppLocalFonts.RESOURCE_BYTES,AppLocalFonts.RESOURCE_BYTES+1})
            assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installResource(new ByteArrayInputStream(new byte[size])));
        byte[] asset;
        try(var stream=AppLocalFontsTest.class.getResourceAsStream(AppLocalFonts.RESOURCE)){asset=stream.readAllBytes();}
        asset[0]^=1;
        assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installResource(new ByteArrayInputStream(asset)));
        final int[] read={0};final boolean[] closed={false};
        InputStream endless=new InputStream(){
            public int read(){if(++read[0]>AppLocalFonts.RESOURCE_BYTES+1)throw new AssertionError("Read beyond cap");return 0;}
            public void close(){closed[0]=true;}
        };
        assertThrows(AppLocalFonts.AssetException.class,()->AppLocalFonts.installResource(endless));
        assertEquals(AppLocalFonts.RESOURCE_BYTES+1,read[0]);assertTrue(closed[0]);
    }

    @Test void absentPolicyKeepsExistingMapperAndParserIdentity()throws Exception {
        assertNull(System.getProperty("workbench.fontPolicy"));
        byte[] fixture=FontFixtures.embedded();
        Path input=root.resolve("default-path.pdf");Files.write(input,fixture);
        Map<String,Object> result=new LinkedHashMap<>();result.put("limitations",new ArrayList<String>());
        try(var fonts=AppLocalFonts.install()) {
            ParseWorker.pdf(input,Protocol.JSON.readTree("{\"limits\":{\"pages\":100}}"),result);
            assertSame(fonts,FontMappers.instance());
            assertEquals("pdfbox-3.0.8",result.get("parser"));
            assertFalse(((List<?>)result.get("limitations")).contains("font_substituted"));
            assertEquals("Embedded \u00e9 \u03a9 \u0416",((String)result.get("text")).strip());
        }
    }

    @Test void freshExtractionsIgnoreSyntheticHostCacheAndKeepTextStable()throws Exception {
        String previous=System.getProperty("pdfbox.fontcache");
        byte[] fixture=FontFixtures.corpus();
        try {
            Path cache=root.resolve("uncreated-font-cache");System.setProperty("pdfbox.fontcache",cache.toString());
            var first=extract(fixture);assertFalse(Files.exists(cache));
            Files.createDirectory(cache);Path sentinel=cache.resolve(".pdfbox.cache");
            byte[] poison="synthetic malformed cache\n".repeat(1000).getBytes(java.nio.charset.StandardCharsets.UTF_8);
            Files.write(sentinel,poison);
            var second=extract(fixture);assertEquals(first.get("text"),second.get("text"));
            assertEquals(first.get("limitations"),second.get("limitations"));assertArrayEquals(poison,Files.readAllBytes(sentinel));
            try(var files=Files.list(cache)){assertEquals(1,files.count());}
        } finally {
            if(previous==null)System.clearProperty("pdfbox.fontcache");else System.setProperty("pdfbox.fontcache",previous);
        }
    }

    @Test void absentAndUnknownPolicyDoNotSilentlySelectLocalFonts()throws Exception {
        assertNull(System.getProperty("workbench.fontPolicy"));assertFalse(AppLocalFonts.selected());
        try {
            for(String invalid:new String[]{"","true","../font",AppLocalFonts.POLICY+"extra"}) {
                System.setProperty("workbench.fontPolicy",invalid);
                assertThrows(AppLocalFonts.AssetException.class,AppLocalFonts::selected);
            }
        } finally {System.clearProperty("workbench.fontPolicy");}
    }
}
