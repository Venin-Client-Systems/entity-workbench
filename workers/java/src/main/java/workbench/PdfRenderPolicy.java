package workbench;

import static workbench.PdfRenderWorker.*;

import java.awt.image.BufferedImage;
import java.io.IOException;
import java.io.InputStream;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.Set;
import java.util.zip.InflaterInputStream;
import javax.imageio.ImageIO;
import javax.imageio.ImageReader;
import javax.imageio.stream.MemoryCacheImageInputStream;
import org.apache.pdfbox.cos.*;

/** Bounded preflight for the supported scan subset, before invoking the PDFBox renderer. */
final class PdfRenderPolicy {
    private static final Set<String> ACTIONS = Set.of(
        "GoTo", "GoToR", "GoToE", "Launch", "Thread", "URI", "Sound", "Movie", "Hide", "Named",
        "SubmitForm", "ResetForm", "ImportData", "JavaScript", "SetOCGState", "Rendition", "Trans", "GoTo3DView");
    private static final Set<String> UNSUPPORTED_KEYS = Set.of(
        "Font", "ExtGState", "Pattern", "Shading", "OCProperties", "OC", "Group", "SMask", "Mask", "AcroForm");
    private final Set<COSBase> visited = Collections.newSetFromMap(new IdentityHashMap<>());
    private long expanded;

    void inspect(COSBase value, int depth) throws IOException {
        if (value == null || !visited.add(value)) {
            return;
        }
        if (depth > 64 || visited.size() > 50000) {
            throw new Refused("quota_exhausted", "structure_limit");
        }
        if (value instanceof COSObject object) {
            inspect(object.getObject(), depth + 1);
        } else if (value instanceof COSArray array) {
            if (array.size() > 50000) {
                throw new Refused("quota_exhausted", "structure_limit");
            }
            for (COSBase item : array) {
                inspect(item, depth + 1);
            }
        } else if (value instanceof COSDictionary dictionary) {
            inspectDictionary(dictionary);
            for (var entry : dictionary.entrySet()) {
                inspect(entry.getValue(), depth + 1);
            }
        }
    }

    private void inspectDictionary(COSDictionary dictionary) throws IOException {
        if (dictionary.size() > 50000) {
            throw new Refused("quota_exhausted", "structure_limit");
        }
        String type = dictionary.getNameAsString(COSName.TYPE, "");
        String subtype = dictionary.getNameAsString(COSName.SUBTYPE, "");
        String action = dictionary.getNameAsString(COSName.S, "");
        if (dictionary.containsKey("OpenAction") || dictionary.containsKey("AA")
                || dictionary.containsKey("JavaScript") || dictionary.containsKey("JS") || ACTIONS.contains(action)) {
            throw new Refused("unsupported", "active_content");
        }
        if (type.equals("Filespec") || dictionary.containsKey("EF") || dictionary.containsKey("Ref")
                || (dictionary instanceof COSStream && (dictionary.containsKey("F")
                    || dictionary.containsKey("FFilter") || dictionary.containsKey("FDecodeParms")))) {
            throw new Refused("unsupported", "external_resource");
        }
        // No font lookup/fallback, form recursion, optional content, masks or advanced graphics.
        if (type.equals("Font") || subtype.equals("Form") || subtype.equals("PS")
                || (type.equals("XObject") && !subtype.equals("Image"))) {
            throw unsupported();
        }
        COSBase xobjects = dictionary.getDictionaryObject(COSName.XOBJECT);
        if (xobjects != null) {
            if (!(xobjects instanceof COSDictionary resources) || resources instanceof COSStream) {
                throw new Refused("failed", "malformed_document");
            }
            for (COSName name : resources.keySet()) {
                COSBase object = resources.getDictionaryObject(name);
                if (!(object instanceof COSStream stream) || !"Image".equals(stream.getNameAsString(COSName.SUBTYPE))) {
                    throw unsupported();
                }
            }
        }
        for (String key : UNSUPPORTED_KEYS) {
            if (dictionary.containsKey(key)) {
                throw unsupported();
            }
        }
        if (subtype.equals("Image")) {
            dimensions(dictionary.getInt(COSName.WIDTH), dictionary.getInt(COSName.HEIGHT));
            String color = dictionary.getNameAsString(COSName.COLORSPACE, "");
            if (!Set.of("DeviceGray", "DeviceRGB").contains(color)
                    || dictionary.getInt(COSName.BITS_PER_COMPONENT) != 8
                    || dictionary.getBoolean(COSName.IMAGE_MASK, false)) {
                throw unsupported();
            }
        }
        if (dictionary instanceof COSStream stream) {
            inspectStream(stream);
        }
    }

    private void inspectStream(COSStream stream) throws IOException {
        COSBase filter = stream.getDictionaryObject(COSName.FILTER);
        // Arrays/chained codecs and optional native codecs are outside this scan profile.
        String name = filter == null ? "" : filter instanceof COSName n ? n.getName() : "unsupported";
        if (!Set.of("", "FlateDecode", "DCTDecode").contains(name)) {
            throw unsupported();
        }
        boolean image = "Image".equals(stream.getNameAsString(COSName.SUBTYPE));
        if (name.equals("DCTDecode") && !image) {
            throw unsupported();
        }
        int width = stream.getInt(COSName.WIDTH);
        int height = stream.getInt(COSName.HEIGHT);
        int components = "DeviceRGB".equals(stream.getNameAsString(COSName.COLORSPACE)) ? 3 : 1;
        int rowPrefix = inspectPredictor(stream, name, image, width, components);
        if (name.equals("DCTDecode")) {
            inspectJpeg(stream, width, height);
        }
        long streamBytes = 0;
        try (InputStream raw = stream.createRawInputStream();
                InputStream decoded = name.equals("FlateDecode") ? new InflaterInputStream(raw) : raw) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = decoded.read(buffer)) != -1) {
                if (image && rowPrefix == 1) {
                    int rowBytes = width * components + 1;
                    for (int offset = 0; offset < count; offset++) {
                        if ((streamBytes + offset) % rowBytes == 0 && (buffer[offset] & 255) > 4) {
                            throw new Refused("failed", "malformed_document");
                        }
                    }
                }
                expanded += count;
                streamBytes += count;
                if (expanded > MAX_EXPANDED) {
                    throw new Refused("quota_exhausted", "stream_limit");
                }
            }
        }
        if (image && !name.equals("DCTDecode")
                && streamBytes != (long) height * ((long) width * components + rowPrefix)) {
            throw new Refused("failed", "malformed_document");
        }
    }

    private int inspectPredictor(COSStream stream, String filter, boolean image, int width, int components)
            throws IOException {
        COSBase parameters = stream.getDictionaryObject(COSName.DECODE_PARMS);
        if (parameters == null) {
            return 0;
        }
        if (!(parameters instanceof COSDictionary p) || !filter.equals("FlateDecode")) {
            throw unsupported();
        }
        int columns = p.getInt(COSName.COLUMNS, 1);
        int colors = p.getInt(COSName.COLORS, 1);
        int bits = p.getInt(COSName.BITS_PER_COMPONENT, 8);
        int predictor = p.getInt(COSName.PREDICTOR, 1);
        if (columns < 1 || columns > MAX_DIMENSION || colors < 1 || colors > 3 || bits != 8
                || !(predictor == 1 || predictor == 2 || (predictor >= 10 && predictor <= 15))) {
            throw unsupported();
        }
        if (!image && predictor != 1) {
            throw unsupported();
        }
        if (image && (columns != width || colors != components)) {
            throw unsupported();
        }
        return predictor >= 10 ? 1 : 0;
    }

    private void inspectJpeg(COSStream stream, int width, int height) throws IOException {
        ImageReader reader = jpegReader();
        try (InputStream raw = stream.createRawInputStream();
                var input = new MemoryCacheImageInputStream(raw)) {
            boolean[] warned = {false};
            reader.addIIOReadWarningListener((source, message) -> warned[0] = true);
            reader.setInput(input, false, true);
            int actualWidth = reader.getWidth(0);
            int actualHeight = reader.getHeight(0);
            dimensions(actualWidth, actualHeight);
            if (actualWidth != width || actualHeight != height || reader.getNumImages(true) != 1) {
                throw new Refused("failed", "malformed_document");
            }
            // Decode once under the same bounds to reject warnings that PDFBox would merely log.
            BufferedImage decoded = reader.read(0);
            if (decoded == null || decoded.getWidth() != width || decoded.getHeight() != height || warned[0]) {
                throw new Refused("failed", "malformed_document");
            }
            decoded.flush();
        } finally {
            reader.dispose();
        }
    }

    private ImageReader jpegReader() throws IOException {
        var candidates = ImageIO.getImageReadersByFormatName("jpeg");
        while (candidates.hasNext()) {
            ImageReader candidate = candidates.next();
            if (candidate.getClass().getName().equals("com.sun.imageio.plugins.jpeg.JPEGImageReader")
                    && "java.desktop".equals(candidate.getClass().getModule().getName())) {
                return candidate;
            }
            candidate.dispose();
        }
        throw new IOException("JDK JPEG reader unavailable");
    }
}
