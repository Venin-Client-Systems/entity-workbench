import java.awt.image.BufferedImage;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import javax.imageio.ImageIO;
import org.apache.pdfbox.cos.*;
import org.apache.pdfbox.pdmodel.*;
import org.apache.pdfbox.pdmodel.common.*;
import org.apache.pdfbox.pdmodel.encryption.*;
import org.apache.pdfbox.pdmodel.graphics.color.PDDeviceRGB;
import org.apache.pdfbox.pdmodel.graphics.image.*;

/** Synthetic only. Generates review, hostile and resource fixtures for the scan subset. */
public final class GeneratePdfRenderFixtures {
    private static Path target;

    private static void save(PDDocument document, String name) throws Exception {
        Path path = target.resolve(name + ".pdf");
        Files.deleteIfExists(path);
        document.save(path.toFile());
    }

    private static PDPage page(PDDocument document) {
        PDPage page = new PDPage(new PDRectangle(600, 115));
        document.addPage(page);
        return page;
    }

    private static void content(PDDocument document, PDPage page, String value) throws Exception {
        PDStream stream = new PDStream(document);
        try (OutputStream output = stream.createOutputStream(COSName.FLATE_DECODE)) {
            output.write(value.getBytes(StandardCharsets.US_ASCII));
        }
        page.setContents(stream);
    }

    private static void imageFixtures(Path imagePath) throws Exception {
        BufferedImage scan = ImageIO.read(imagePath.toFile());
        try (PDDocument document = new PDDocument()) {
            PDPage page = page(document);
            try (PDPageContentStream output = new PDPageContentStream(document, page)) {
                output.drawImage(LosslessFactory.createFromImage(document, scan), 0, 0, 600, 115);
            }
            content(document, page(document), "0 g 0 0 600 115 re f");
            save(document, "scan");
        }
        for (String mode : new String[] {"scan-jpeg", "jpeg-dimensions", "jpeg-pixel-limit", "short-image", "long-image"}) {
            try (PDDocument document = new PDDocument()) {
                PDPage page = page(document);
                PDImageXObject image;
                if (mode.equals("short-image") || mode.equals("long-image")) {
                    image = new PDImageXObject(document);
                    image.setWidth(10);
                    image.setHeight(10);
                    image.setBitsPerComponent(8);
                    image.setColorSpace(PDDeviceRGB.INSTANCE);
                    try (OutputStream output = image.getCOSObject().createOutputStream(COSName.FLATE_DECODE)) {
                        output.write(new byte[mode.equals("short-image") ? 1 : 301]);
                    }
                } else {
                    String fixture = mode.equals("jpeg-pixel-limit") ? "oversize.jpg" : "synthetic.jpg";
                    byte[] jpeg = Files.readAllBytes(imagePath.resolveSibling(fixture));
                    image = new PDImageXObject(document, new ByteArrayInputStream(jpeg),
                        COSName.DCT_DECODE, 1200, 230, 8, PDDeviceRGB.INSTANCE);
                    if (mode.equals("jpeg-dimensions")) {
                        image.setWidth(1199);
                    }
                }
                try (PDPageContentStream output = new PDPageContentStream(document, page)) {
                    output.drawImage(image, 0, 0, 600, 115);
                }
                save(document, mode);
            }
        }
    }

    private static void hostileFixture(String mode) throws Exception {
        try (PDDocument document = new PDDocument()) {
            PDPage page = page(document);
            switch (mode) {
                case "large-page" -> page.setMediaBox(new PDRectangle(9000, 9000));
                case "active" -> {
                    COSDictionary action = new COSDictionary();
                    action.setName(COSName.S, "JavaScript");
                    action.setString(COSName.getPDFName("JS"), "throw new Error('synthetic script must not run')");
                    document.getDocumentCatalog().getCOSObject().setItem(COSName.OPEN_ACTION, action);
                }
                case "external" -> {
                    COSStream stream = document.getDocument().createCOSStream();
                    stream.setString(COSName.F, "synthetic-outside-sentinel");
                    document.getDocumentCatalog().getCOSObject().setItem(COSName.getPDFName("SyntheticExternal"), stream);
                }
                case "font" -> {
                    COSDictionary font = new COSDictionary();
                    font.setName(COSName.TYPE, "Font");
                    font.setName(COSName.SUBTYPE, "Type1");
                    font.setName(COSName.BASE_FONT, "Helvetica");
                    PDResources resources = new PDResources();
                    COSDictionary fonts = new COSDictionary();
                    fonts.setItem(COSName.getPDFName("F1"), font);
                    resources.getCOSObject().setItem(COSName.FONT, fonts);
                    page.setResources(resources);
                    content(document, page, "BT /F1 12 Tf (synthetic font) Tj ET");
                }
                case "user-unit" -> page.getCOSObject().setFloat(COSName.USER_UNIT, 2);
                case "invalid-rotation" -> page.setRotation(45);
                case "missing-image" -> content(document, page, "/Missing Do");
                case "unknown-operator" -> content(document, page, "syntheticUnknown");
                case "operator-limit" -> content(document, page, "n\n".repeat(100001));
                case "structure-limit" -> {
                    COSArray nested = new COSArray();
                    for (int depth = 0; depth < 70; depth++) {
                        COSArray parent = new COSArray();
                        parent.add(nested);
                        nested = parent;
                    }
                    document.getDocumentCatalog().getCOSObject().setItem(COSName.getPDFName("SyntheticDepth"), nested);
                }
                case "stream-limit" -> {
                    COSStream stream = document.getDocument().createCOSStream();
                    try (OutputStream output = stream.createOutputStream(COSName.FLATE_DECODE)) {
                        byte[] zeros = new byte[1024 * 1024];
                        for (int n = 0; n < 33; n++) {
                            output.write(zeros);
                        }
                    }
                    document.getDocumentCatalog().getCOSObject().setItem(COSName.getPDFName("SyntheticExpansion"), stream);
                }
                case "unsupported-filter" -> {
                    PDStream stream = new PDStream(document);
                    try (OutputStream output = stream.createOutputStream()) {
                        output.write(new byte[] {0});
                    }
                    stream.getCOSObject().setItem(COSName.FILTER, COSName.LZW_DECODE);
                    page.setContents(stream);
                }
                default -> throw new IllegalArgumentException("Unknown synthetic mode");
            }
            save(document, mode);
        }
    }

    public static void main(String[] args) throws Exception {
        target = Path.of(args[0]);
        Files.createDirectories(target);
        imageFixtures(Path.of(args[1]));
        for (int rotation : new int[] {0, 90, 180, 270}) {
            try (PDDocument document = new PDDocument()) {
                PDPage page = new PDPage(new PDRectangle(100, 80));
                document.addPage(page);
                page.setCropBox(new PDRectangle(10.25f, 20.5f, 80.75f, 40.25f));
                page.setRotation(rotation);
                content(document, page, "0 g 10.25 40.75 20 20 re f");
                save(document, "crop-" + rotation);
            }
        }
        try (PDDocument document = new PDDocument()) {
            PDPage page = new PDPage(new PDRectangle(-50, -50, 150, 150));
            document.addPage(page);
            page.setCropBox(new PDRectangle(-10.1f, -20.2f, 80.75f, 40.25f));
            page.setRotation(270);
            content(document, page, "0 g -10 0 20 20 re f");
            save(document, "crop-cross-zero");
        }
        for (String mode : new String[] {"large-page", "active", "external", "font", "user-unit",
                "invalid-rotation", "missing-image", "unknown-operator", "operator-limit", "stream-limit", "structure-limit", "unsupported-filter"}) {
            hostileFixture(mode);
        }
        try (PDDocument document = new PDDocument()) {
            for (int n = 0; n < 1001; n++) {
                page(document);
            }
            save(document, "page-limit");
        }
        for (String password : new String[] {"synthetic-password", ""}) {
            try (PDDocument document = new PDDocument()) {
                page(document);
                StandardProtectionPolicy policy = new StandardProtectionPolicy("synthetic-owner", password, new AccessPermission());
                policy.setEncryptionKeyLength(128);
                document.protect(policy);
                save(document, password.isEmpty() ? "encrypted-empty" : "encrypted");
            }
        }
        Files.writeString(target.resolve("malformed.pdf"), "%PDF-1.7\nsynthetic invalid PDF\n%%EOF\n");
    }
}
