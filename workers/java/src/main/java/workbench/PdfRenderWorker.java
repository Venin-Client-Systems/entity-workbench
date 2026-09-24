package workbench;

import com.fasterxml.jackson.databind.JsonNode;
import java.awt.Graphics2D;
import java.awt.geom.AffineTransform;
import java.awt.image.BufferedImage;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import org.apache.fontbox.FontBoxFont;
import org.apache.fontbox.ttf.TrueTypeFont;
import org.apache.pdfbox.contentstream.operator.Operator;
import org.apache.pdfbox.cos.*;
import org.apache.pdfbox.io.RandomAccessReadBuffer;
import org.apache.pdfbox.pdfparser.PDFParser;
import org.apache.pdfbox.pdmodel.*;
import org.apache.pdfbox.pdmodel.common.PDRectangle;
import org.apache.pdfbox.pdmodel.encryption.InvalidPasswordException;
import org.apache.pdfbox.pdmodel.font.*;
import org.apache.pdfbox.pdmodel.graphics.image.*;
import org.apache.pdfbox.rendering.*;

/** One explicitly selected scan page per confined JVM. No canonical writes or document scripts. */
public final class PdfRenderWorker {
    static final int MAX_DIMENSION = 8192;
    static final int MAX_PAGES = 1000;
    static final long MAX_PIXELS = 12_000_000;
    static final long MAX_EXPANDED = 32 * 1024 * 1024;

    static final class Refused extends IOException {
        final String status;
        final String code;
        Refused(String status, String code) {
            this.status = status;
            this.code = code;
        }
    }

    static Refused unsupported() {
        return new Refused("unsupported", "unsupported_feature");
    }

    private static String hash(byte[] bytes) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }

    static void dimensions(int width, int height) throws Refused {
        if (width < 1 || height < 1) {
            throw new Refused("failed", "malformed_document");
        }
        if (width > MAX_DIMENSION || height > MAX_DIMENSION || (long) width * height > MAX_PIXELS) {
            throw new Refused("quota_exhausted", "pixel_limit");
        }
    }

    /** Reject PDFBox's recover-and-log defaults: missing resources must fail the whole raster. */
    private static final class StrictDrawer extends PageDrawer {
        private int operators;
        private double[] pageMapping;
        private static final Set<String> ALLOWED = Set.of(
            "q", "Q", "cm", "Do", "re", "f", "F", "f*", "n", "m", "l", "c", "v", "y", "h", "S", "s",
            "B", "B*", "b", "b*", "W", "W*", "w", "J", "j", "M", "d", "G", "g", "RG", "rg");

        StrictDrawer(PageDrawerParameters parameters) throws IOException {
            super(parameters);
        }

        @Override
        public void drawPage(Graphics2D graphics, PDRectangle box) throws IOException {
            // Capture the renderer's actual scale/rotation and apply the exact PageDrawer
            // translations. Width/height are float-rounded before these double operations.
            AffineTransform transform = new AffineTransform(graphics.getTransform());
            transform.translate(0, box.getHeight());
            transform.scale(1, -1);
            transform.translate(-box.getLowerLeftX(), -box.getLowerLeftY());
            pageMapping = new double[6];
            transform.getMatrix(pageMapping);
            super.drawPage(graphics, box);
        }

        @Override
        protected void processOperator(Operator operator, List<COSBase> operands) throws IOException {
            if (++operators > 100000) {
                throw new Refused("quota_exhausted", "operator_limit");
            }
            if (!ALLOWED.contains(operator.getName())) {
                throw unsupported();
            }
            super.processOperator(operator, operands);
        }

        @Override
        protected void operatorException(Operator operator, List<COSBase> operands, IOException error)
                throws IOException {
            throw error;
        }

        @Override
        protected void unsupportedOperator(Operator operator, List<COSBase> operands) throws IOException {
            throw unsupported();
        }

        @Override
        public void drawImage(PDImage image) throws IOException {
            if (!(image instanceof PDImageXObject)) {
                throw unsupported();
            }
            dimensions(image.getWidth(), image.getHeight());
            super.drawImage(image);
        }
    }

    /** Fail even if a future unreviewed engine path attempts font fallback despite preflight. */
    private static final class NoFontMapper implements FontMapper {
        public FontMapping<TrueTypeFont> getTrueTypeFont(String name, PDFontDescriptor descriptor) {
            throw new IllegalStateException("Font substitution disabled");
        }
        public FontMapping<FontBoxFont> getFontBoxFont(String name, PDFontDescriptor descriptor) {
            throw new IllegalStateException("Font substitution disabled");
        }
        public CIDFontMapping getCIDFont(String name, PDFontDescriptor descriptor, PDCIDSystemInfo info) {
            throw new IllegalStateException("Font substitution disabled");
        }
    }

    private static final class StrictRenderer extends PDFRenderer {
        private StrictDrawer drawer;

        StrictRenderer(PDDocument document) {
            super(document);
        }

        @Override
        protected PageDrawer createPageDrawer(PageDrawerParameters parameters) throws IOException {
            drawer = new StrictDrawer(parameters);
            return drawer;
        }

        double[] pageMapping() throws IOException {
            if (drawer == null || drawer.pageMapping == null) {
                throw new IOException("Missing page transform");
            }
            return drawer.pageMapping;
        }
    }

    private static void validatePage(PDPage page) throws IOException {
        COSBase unit = page.getCOSObject().getDictionaryObject(COSName.USER_UNIT);
        if (unit != null && (!(unit instanceof COSNumber n) || n.floatValue() != 1)) {
            throw unsupported();
        }
        COSBase rotate = PDPageTree.getInheritableAttribute(page.getCOSObject(), COSName.ROTATE);
        if (rotate != null && (!(rotate instanceof COSNumber n) || !Float.isFinite(n.floatValue())
                || n.floatValue() % 90 != 0 || Math.abs(n.floatValue()) > 3600)) {
            throw unsupported();
        }
    }

    private static byte[] raster(BufferedImage image) {
        int width = image.getWidth(), height = image.getHeight();
        byte[] header = ("P5\n" + width + " " + height + "\n255\n").getBytes(StandardCharsets.US_ASCII);
        byte[] bytes = new byte[header.length + width * height];
        System.arraycopy(header, 0, bytes, 0, header.length);
        int offset = header.length;
        for (int y = 0; y < height; y++) {
            for (int x = 0; x < width; x++) {
                int rgb = image.getRGB(x, y);
                bytes[offset++] = (byte) ((299 * ((rgb >> 16) & 255)
                    + 587 * ((rgb >> 8) & 255) + 114 * (rgb & 255) + 500) / 1000);
            }
        }
        return bytes;
    }

    private static void render(byte[] original, int pageNumber, int dpi, Map<String, Object> result)
            throws Exception {
        String header = new String(original, 0, Math.min(original.length, 5), StandardCharsets.US_ASCII);
        if (!header.equals("%PDF-")) {
            throw new Refused("unsupported", "unsupported_format");
        }
        FontMappers.set(new NoFontMapper());
        try (var input = new RandomAccessReadBuffer(original); var document = new PDFParser(input).parse(false)) {
            if (document.isEncrypted()) {
                throw new Refused("encrypted", "encrypted_document");
            }
            int count = document.getNumberOfPages();
            if (count < 1) {
                throw new Refused("failed", "malformed_document");
            }
            result.put("page_count", count);
            if (count > MAX_PAGES) {
                throw new Refused("quota_exhausted", "page_limit");
            }
            if (pageNumber > count) {
                throw new Refused("failed", "page_out_of_range");
            }
            new PdfRenderPolicy().inspect(document.getDocument().getTrailer(), 0);
            PDPage page = document.getPage(pageNumber - 1);
            validatePage(page);
            PDRectangle box = page.getCropBox();
            float[] crop = {box.getLowerLeftX(), box.getLowerLeftY(), box.getUpperRightX(), box.getUpperRightY()};
            for (float coordinate : crop) {
                if (!Float.isFinite(coordinate) || Math.abs(coordinate) > 1_000_000) {
                    throw new Refused("failed", "malformed_document");
                }
            }
            if (box.getWidth() <= 0 || box.getHeight() <= 0) {
                throw new Refused("failed", "malformed_document");
            }
            float scale = dpi / 72f;
            int width = (int) Math.max(Math.floor(box.getWidth() * scale), 1);
            int height = (int) Math.max(Math.floor(box.getHeight() * scale), 1);
            dimensions(width, height);
            int rotation = page.getRotation();
            if (rotation == 90 || rotation == 270) {
                int swap = width;
                width = height;
                height = swap;
            }
            StrictRenderer renderer = new StrictRenderer(document);
            renderer.setAnnotationsFilter(annotation -> false);
            renderer.setSubsamplingAllowed(false);
            BufferedImage image = renderer.renderImageWithDPI(pageNumber - 1, dpi, ImageType.RGB);
            if (image.getWidth() != width || image.getHeight() != height) {
                throw new IOException("Unexpected raster dimensions");
            }
            byte[] bytes = raster(image);
            image.flush();
            result.put("geometry", Map.of("crop_box", new double[] {crop[0], crop[1], crop[2], crop[3]},
                "rotation_degrees", rotation, "pdf_to_raster", renderer.pageMapping()));
            result.put("raster", Map.of("path", "raster.pgm", "sha256", hash(bytes),
                "bytes", bytes.length, "width", width, "height", height));
            Files.write(Path.of("raster.pgm"), bytes, StandardOpenOption.CREATE_NEW);
        }
    }

    private static void validateRequest(JsonNode request) throws IOException {
        if (!request.path("operation").asText().equals("parse") || request.path("inputs").size() != 1
                || !request.path("inputs").get(0).asText().equals("input.json")
                || !request.path("output").asText().equals("result.json")) {
            throw new IOException("Invalid renderer assignment");
        }
        JsonNode limits = request.path("limits");
        if (limits.path("pages").asInt() != MAX_PAGES || limits.path("pixels").asLong() != MAX_PIXELS
                || limits.path("expanded_bytes").asLong() != MAX_EXPANDED
                || limits.path("output_bytes").asLong() != 8192 || limits.path("seconds").asInt() != 30) {
            throw new IOException("Invalid rendering limits");
        }
    }

    private static Map<String, Object> result(JsonNode request, byte[] original, int page, int dpi)
            throws Exception {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("protocol_version", 1);
        result.put("job_id", request.path("job_id").asText());
        result.put("original_sha256", hash(original));
        result.put("original_bytes", original.length);
        result.put("renderer", "pdfbox-3.0.8-scan-v1");
        result.put("java_runtime", System.getProperty("java.runtime.version"));
        result.put("page_number", page);
        result.put("dpi", dpi);
        result.put("page_count", null);
        result.put("status", "rendered");
        result.put("failure", null);
        result.put("geometry", null);
        result.put("raster", null);
        result.put("limitations", List.of("scan_focused_subset", "annotations_excluded",
            "color_converted_to_gray", "unreviewed_raster", "no_word_regions"));
        return result;
    }

    public static void main(String[] args) {
        try {
            if (args.length != 2) {
                throw new IOException("Page and DPI required");
            }
            int page = Integer.parseInt(args[0]), dpi = Integer.parseInt(args[1]);
            if (page < 1 || page > MAX_PAGES || dpi < 72 || dpi > 300) {
                throw new IOException("Invalid render settings");
            }
            JsonNode request = Protocol.read();
            validateRequest(request);
            byte[] original = Files.readAllBytes(Protocol.input("input.json"));
            Map<String, Object> result = result(request, original, page, dpi);
            try {
                render(original, page, dpi, result);
            } catch (InvalidPasswordException error) {
                result.put("status", "encrypted");
                result.put("failure", "encrypted_document");
                result.put("page_count", null);
            } catch (Refused error) {
                result.put("status", error.status);
                result.put("failure", error.code);
            } catch (IOException | RuntimeException error) {
                result.put("status", "failed");
                result.put("failure", "malformed_document");
            }
            if (!result.get("status").equals("rendered")) {
                result.put("geometry", null);
                result.put("raster", null);
                Files.deleteIfExists(Path.of("raster.pgm"));
            }
            Protocol.write(request, result);
        } catch (Exception error) {
            Protocol.failure(error);
        }
    }
}
