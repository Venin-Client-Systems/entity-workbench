package workbench;

import com.fasterxml.jackson.databind.JsonNode;
import java.awt.image.BufferedImage;
import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.HexFormat;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.zip.CRC32;
import javax.imageio.IIOException;
import javax.imageio.ImageIO;
import javax.imageio.ImageReader;
import javax.imageio.stream.MemoryCacheImageInputStream;

/** One PNG/JPEG raster job per confined process; no canonical writes or OCR execution. */
public final class ImageWorker {
    private static final int MAX_DIMENSION = 8192;
    private static final long MAX_PIXELS = 12_000_000;
    private static final byte[] PNG_SIGNATURE = {(byte)137, 80, 78, 71, 13, 10, 26, 10};
    private static final Set<String> ANIMATION_CHUNKS = Set.of("acTL", "fcTL", "fdAT");

    private static final class Refused extends IOException {
        final String status;
        final String code;
        Refused(String status, String code) {
            this.status = status;
            this.code = code;
        }
    }

    private record Raster(byte[] bytes, int width, int height) {}

    private static String hash(byte[] bytes) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }

    private static String format(byte[] bytes) {
        if (bytes.length >= 8 && Arrays.equals(Arrays.copyOf(bytes, 8), PNG_SIGNATURE)) {
            return "png";
        }
        if (bytes.length >= 3 && (bytes[0] & 255) == 255
                && (bytes[1] & 255) == 216 && (bytes[2] & 255) == 255) {
            return "jpeg";
        }
        return null;
    }

    private static void dimensions(int width, int height, long limit) throws Refused {
        if (width < 1 || height < 1) {
            throw new Refused("failed", "malformed_image");
        }
        if (width > MAX_DIMENSION || height > MAX_DIMENSION || (long)width * height > limit) {
            throw new Refused("quota_exhausted", "pixel_limit");
        }
    }

    private static int integer(byte[] bytes, int offset) {
        return ((bytes[offset] & 255) << 24) | ((bytes[offset + 1] & 255) << 16)
            | ((bytes[offset + 2] & 255) << 8) | (bytes[offset + 3] & 255);
    }

    /** Bound the container walk and reject APNG rather than silently treating it as a still. */
    private static void pngContainer(byte[] bytes, long pixels) throws Refused {
        int offset = 8;
        int count = 0;
        boolean header = false;
        boolean end = false;
        while (offset < bytes.length) {
            if (++count > 1024) {
                throw new Refused("quota_exhausted", "container_limit");
            }
            if (bytes.length - offset < 12) {
                throw new Refused("failed", "malformed_image");
            }
            int length = integer(bytes, offset);
            if (length < 0 || length > bytes.length - offset - 12) {
                throw new Refused("failed", "malformed_image");
            }
            CRC32 crc = new CRC32();
            crc.update(bytes, offset + 4, length + 4);
            if (crc.getValue() != Integer.toUnsignedLong(integer(bytes, offset + 8 + length))) {
                throw new Refused("failed", "malformed_image");
            }
            String type = new String(bytes, offset + 4, 4, StandardCharsets.US_ASCII);
            if (!header) {
                if (!type.equals("IHDR") || length != 13) {
                    throw new Refused("failed", "malformed_image");
                }
                dimensions(integer(bytes, offset + 8), integer(bytes, offset + 12), pixels);
                header = true;
            } else if (type.equals("IHDR")) {
                throw new Refused("failed", "malformed_image");
            }
            if (ANIMATION_CHUNKS.contains(type)) {
                throw new Refused("unsupported", "multiple_images");
            }
            offset += length + 12;
            if (type.equals("IEND")) {
                if (length != 0 || offset != bytes.length) {
                    throw new Refused("failed", "malformed_image");
                }
                end = true;
                break;
            }
        }
        if (!end) {
            throw new Refused("failed", "malformed_image");
        }
    }

    private static ImageReader reader(String format) throws IOException {
        String expected = format.equals("png")
            ? "com.sun.imageio.plugins.png.PNGImageReader"
            : "com.sun.imageio.plugins.jpeg.JPEGImageReader";
        Iterator<ImageReader> readers = ImageIO.getImageReadersByFormatName(format);
        while (readers.hasNext()) {
            ImageReader candidate = readers.next();
            if (candidate.getClass().getName().equals(expected)
                    && "java.desktop".equals(candidate.getClass().getModule().getName())) {
                return candidate;
            }
            candidate.dispose();
        }
        throw new IOException("Required JDK image reader unavailable");
    }

    private static int whiteComposite(int color, int alpha) {
        return (color * alpha + 255 * (255 - alpha) + 127) / 255;
    }

    private static Raster decode(byte[] original, String format, long pixels) throws Exception {
        if (format.equals("png")) {
            pngContainer(original, pixels);
        }
        ImageReader reader = reader(format);
        try (var stream = new MemoryCacheImageInputStream(new ByteArrayInputStream(original))) {
            boolean[] warning = {false};
            reader.addIIOReadWarningListener((source, message) -> warning[0] = true);
            reader.setInput(stream, false, true);
            int width = reader.getWidth(0);
            int height = reader.getHeight(0);
            dimensions(width, height, pixels);
            if (reader.getNumImages(true) != 1) {
                throw new Refused("unsupported", "multiple_images");
            }
            BufferedImage image = reader.read(0);
            if (warning[0]) {
                throw new Refused("failed", "decoder_warning");
            }
            if (image == null || image.getWidth() != width || image.getHeight() != height) {
                throw new Refused("failed", "malformed_image");
            }
            byte[] header = ("P5\n" + width + " " + height + "\n255\n")
                .getBytes(StandardCharsets.US_ASCII);
            byte[] bytes = new byte[header.length + width * height];
            System.arraycopy(header, 0, bytes, 0, header.length);
            int[] row = new int[width];
            int offset = header.length;
            for (int y = 0; y < height; y++) {
                image.getRGB(0, y, width, 1, row, 0, width);
                for (int argb : row) {
                    int alpha = (argb >>> 24) & 255;
                    int red = whiteComposite((argb >>> 16) & 255, alpha);
                    int green = whiteComposite((argb >>> 8) & 255, alpha);
                    int blue = whiteComposite(argb & 255, alpha);
                    bytes[offset++] = (byte)((299 * red + 587 * green + 114 * blue + 500) / 1000);
                }
            }
            image.flush();
            return new Raster(bytes, width, height);
        } finally {
            reader.dispose();
        }
    }

    private static long validateRequest(JsonNode request) throws IOException {
        if (!request.path("operation").asText().equals("parse")
                || request.path("inputs").size() != 1
                || !request.path("output").asText().equals("result.json")) {
            throw new IOException("Invalid image request");
        }
        JsonNode limits = request.path("limits");
        long pixels = limits.path("pixels").asLong();
        if (pixels < 1 || pixels > MAX_PIXELS || limits.path("pages").asInt() != 1
                || limits.path("expanded_bytes").asLong() != MAX_PIXELS + 32) {
            throw new IOException("Invalid image limits");
        }
        return pixels;
    }

    private static Map<String, Object> result(JsonNode request, byte[] original, String format)
            throws Exception {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("protocol_version", 1);
        result.put("job_id", request.path("job_id").asText());
        result.put("original_sha256", hash(original));
        result.put("original_bytes", original.length);
        result.put("decoder", "jdk-imageio-21-v1");
        result.put("java_runtime", System.getProperty("java.runtime.version"));
        result.put("media_type", format == null ? "application/octet-stream" : "image/" + format);
        result.put("status", "unsupported");
        result.put("failure", "unsupported_format");
        result.put("raster", null);
        result.put("limitations", List.of("exif_orientation_not_applied", "embedded_previews_excluded",
            "metadata_not_extracted", "color_converted_to_gray", "no_document_page_mapping"));
        return result;
    }

    public static void main(String[] args) {
        try {
            JsonNode request = Protocol.read();
            long pixels = validateRequest(request);
            byte[] original = Files.readAllBytes(Protocol.input(request.path("inputs").get(0).asText()));
            String format = format(original);
            Map<String, Object> result = result(request, original, format);
            if (format != null) {
                try {
                    Raster raster = decode(original, format, pixels);
                    if (raster.bytes.length > MAX_PIXELS + 32) {
                        throw new Refused("quota_exhausted", "pixel_limit");
                    }
                    Files.write(Path.of("raster.pgm"), raster.bytes, StandardOpenOption.CREATE_NEW);
                    result.put("status", "decoded");
                    result.put("failure", null);
                    result.put("raster", Map.of(
                        "path", "raster.pgm", "sha256", hash(raster.bytes), "bytes", raster.bytes.length,
                        "width", raster.width, "height", raster.height, "source_image_index", 0,
                        "pixel_mapping", "encoded_pixels_gray_white_alpha_v1"));
                } catch (Refused refused) {
                    result.put("status", refused.status);
                    result.put("failure", refused.code);
                } catch (IIOException | IllegalArgumentException malformed) {
                    result.put("status", "failed");
                    result.put("failure", "malformed_image");
                }
            }
            Protocol.write(request, result);
        } catch (Exception error) {
            Protocol.failure(error);
        }
    }
}
