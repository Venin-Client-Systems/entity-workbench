package workbench;

import java.io.IOException;
import java.io.InputStream;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.Enumeration;
import java.net.URL;
import org.apache.fontbox.FontBoxFont;
import org.apache.fontbox.ttf.TTFParser;
import org.apache.fontbox.ttf.TrueTypeFont;
import org.apache.pdfbox.io.RandomAccessReadBuffer;
import org.apache.pdfbox.pdmodel.font.*;

/** Fixed extraction fallback. One document/job per process; no host font lookup. */
final class AppLocalFonts implements FontMapper, AutoCloseable {
    static final String POLICY = "liberation-sans-2.1.5-extraction-v1";
    static final String PARSER = "pdfbox-3.0.8-local-fonts-v1";
    static final String RESOURCE = "/org/apache/pdfbox/resources/ttf/LiberationSans-Regular.ttf";
    static final int RESOURCE_BYTES = 410_712;
    static final String SHA256 = "76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8";

    static final class AssetException extends IOException {
        AssetException() { super("App-local font policy or asset rejected"); }
    }

    private final TrueTypeFont font;
    private boolean used;

    private AppLocalFonts(TrueTypeFont font) { this.font = font; }

    static boolean selected() throws AssetException {
        String value = System.getProperty("workbench.fontPolicy");
        if (value == null) return false;
        if (!POLICY.equals(value)) throw new AssetException();
        return true;
    }

    static AppLocalFonts install() throws IOException {
        ClassLoader loader=AppLocalFonts.class.getClassLoader();
        if(loader==null)throw new AssetException();
        try { return installCandidates(loader.getResources(RESOURCE.substring(1))); }
        catch(IOException unavailable) { throw new AssetException(); }
    }

    static AppLocalFonts installCandidates(Enumeration<URL> candidates) throws IOException {
        if(!candidates.hasMoreElements())throw new AssetException();
        URL only=candidates.nextElement();
        // Stop as soon as a second fixed-name resource is present. Never pick
        // the first classpath match or open an ambiguous candidate.
        if(candidates.hasMoreElements())throw new AssetException();
        return installResource(only.openStream());
    }

    static AppLocalFonts installResource(InputStream resource) throws IOException {
        if (resource == null) throw new AssetException();
        byte[] bytes;
        try (resource) { bytes = resource.readNBytes(RESOURCE_BYTES + 1); }
        catch (IOException unreadable) { throw new AssetException(); }
        if (bytes.length != RESOURCE_BYTES) throw new AssetException();
        try {
            String hash = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
            if (!SHA256.equals(hash)) throw new AssetException();
        } catch (NoSuchAlgorithmException unavailable) { throw new AssetException(); }
        RandomAccessReadBuffer buffer = new RandomAccessReadBuffer(bytes);
        try {
            AppLocalFonts mapper = new AppLocalFonts(new TTFParser().parse(buffer));
            FontMappers.set(mapper);
            return mapper;
        } catch (IOException | RuntimeException malformed) {
            buffer.close();
            throw new AssetException();
        }
    }

    boolean used() { return used; }

    public FontMapping<TrueTypeFont> getTrueTypeFont(String name, PDFontDescriptor descriptor) {
        used = true;
        return new FontMapping<>(font, true);
    }

    public FontMapping<FontBoxFont> getFontBoxFont(String name, PDFontDescriptor descriptor) {
        used = true;
        return new FontMapping<>(font, true);
    }

    public CIDFontMapping getCIDFont(String name, PDFontDescriptor descriptor, PDCIDSystemInfo info) {
        used = true;
        return new CIDFontMapping(null, font, true);
    }

    public void close() throws IOException {
        // This worker runs one job. Clear our process-local mapper without
        // retrieving/initializing the default mapper or its filesystem cache.
        FontMappers.set(null);
        font.close();
    }
}
