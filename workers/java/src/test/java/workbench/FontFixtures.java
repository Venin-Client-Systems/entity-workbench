package workbench;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import org.apache.pdfbox.cos.*;
import org.apache.pdfbox.pdmodel.*;
import org.apache.pdfbox.pdmodel.font.*;
import org.apache.pdfbox.pdmodel.font.Standard14Fonts.FontName;

/** Synthetic fixtures only; this class is excluded from both worker runtimes. */
final class FontFixtures {
    static byte[] embedded() throws Exception {
        try (AppLocalFonts fonts=AppLocalFonts.install(); PDDocument document=new PDDocument()) {
            embeddedPage(document);
            return save(document);
        }
    }

    static byte[] corpus() throws Exception {
        try (AppLocalFonts fonts=AppLocalFonts.install(); PDDocument document=new PDDocument()) {
            for (FontName name:FontName.values()) {
                String text=switch(name) {
                    case SYMBOL -> "\u0391\u0392";
                    case ZAPF_DINGBATS -> "\u2701\u2702";
                    default -> "Standard14 caf\u00e9 123 \u20ac " + name.name();
                };
                page(document,new PDType1Font(name),text);
                markLastPage(document,"STD_"+name.name());
            }
            embeddedPage(document);markLastPage(document,"EMBEDDED");
            page(document,new PDType1Font(simple("SyntheticUnknown",true)),"Unknown explicit widths");markLastPage(document,"UNKNOWN_WIDTHS");
            page(document,new PDType1Font(simple("../../private/SyntheticUnknown",false)),"Unknown missing widths");markLastPage(document,"UNKNOWN_NO_WIDTHS");
            COSDictionary differences=simple("SyntheticDifferences",true);
            COSDictionary encoding=new COSDictionary();
            encoding.setItem(COSName.BASE_ENCODING,COSName.WIN_ANSI_ENCODING);
            COSArray values=new COSArray(); values.add(COSInteger.get(65));
            for(String glyph:new String[]{"eacute","fi","Gamma"})values.add(COSName.getPDFName(glyph));
            encoding.setItem(COSName.DIFFERENCES,values);differences.setItem(COSName.ENCODING,encoding);
            codes(document,new PDType1Font(differences),"414243");markLastPage(document,"DIFFERENCES");
            COSDictionary unicode=simple("SyntheticUnicode",true);
            unicode.setItem(COSName.TO_UNICODE,cmap(document,false));
            codes(document,new PDType1Font(unicode),"414243");markLastPage(document,"SIMPLE_UNICODE");
            codes(document,new PDType0Font(cid(document,true)),"000100020003");markLastPage(document,"CID_UNICODE");
            codes(document,new PDType0Font(cid(document,false)),"0001");markLastPage(document,"CID_UNMAPPED");
            return save(document);
        }
    }

    static COSDictionary simple(String name,boolean widths) {
        COSDictionary font=new COSDictionary();
        font.setItem(COSName.TYPE,COSName.FONT);font.setItem(COSName.SUBTYPE,COSName.TYPE1);
        font.setName(COSName.BASE_FONT,name);font.setItem(COSName.ENCODING,COSName.WIN_ANSI_ENCODING);
        if(widths) {
            font.setInt(COSName.FIRST_CHAR,32);font.setInt(COSName.LAST_CHAR,255);
            COSArray values=new COSArray();for(int i=32;i<=255;i++)values.add(COSInteger.get(600));
            font.setItem(COSName.WIDTHS,values);
        }
        return font;
    }

    private static COSDictionary cid(PDDocument document,boolean unicode)throws IOException {
        COSDictionary descriptor=new COSDictionary();descriptor.setItem(COSName.TYPE,COSName.FONT_DESC);
        descriptor.setName(COSName.FONT_NAME,"SyntheticCID");descriptor.setInt(COSName.FLAGS,4);
        COSArray bbox=new COSArray();for(int n:new int[]{0,-200,1000,900})bbox.add(COSInteger.get(n));
        descriptor.setItem(COSName.FONT_BBOX,bbox);
        COSDictionary system=new COSDictionary();system.setString(COSName.REGISTRY,"Adobe");
        system.setString(COSName.ORDERING,"Identity");system.setInt(COSName.SUPPLEMENT,0);
        COSDictionary child=new COSDictionary();child.setItem(COSName.TYPE,COSName.FONT);
        child.setItem(COSName.SUBTYPE,COSName.CID_FONT_TYPE2);child.setName(COSName.BASE_FONT,"SyntheticCID");
        child.setItem(COSName.FONT_DESC,descriptor);child.setItem(COSName.CIDSYSTEMINFO,system);
        child.setItem(COSName.CID_TO_GID_MAP,COSName.IDENTITY);child.setInt(COSName.DW,600);
        COSDictionary font=new COSDictionary();font.setItem(COSName.TYPE,COSName.FONT);
        font.setItem(COSName.SUBTYPE,COSName.TYPE0);font.setName(COSName.BASE_FONT,"SyntheticCID");
        font.setItem(COSName.ENCODING,COSName.IDENTITY_H);COSArray descendants=new COSArray();descendants.add(child);
        font.setItem(COSName.DESCENDANT_FONTS,descendants);
        if(unicode)font.setItem(COSName.TO_UNICODE,cmap(document,true));
        return font;
    }

    private static COSStream cmap(PDDocument document,boolean cid)throws IOException {
        String range=cid?"<0000> <ffff>":"<00> <ff>";
        String entries=cid?"<0001> <4e00>\n<0002> <d83dde00>\n<0003> <0627>":"<41> <4e00>\n<42> <d83dde00>\n<43> <0627>";
        String content="/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
            +"/CIDSystemInfo << /Registry (Synthetic) /Ordering (Unicode) /Supplement 0 >> def\n"
            +"/CMapName /SyntheticUnicode def /CMapType 2 def\n1 begincodespacerange\n"+range
            +"\nendcodespacerange\n3 beginbfchar\n"+entries+"\nendbfchar\nendcmap\n"
            +"CMapName currentdict /CMap defineresource pop end end\n";
        COSStream stream=document.getDocument().createCOSStream();
        try(OutputStream output=stream.createOutputStream()){output.write(content.getBytes(StandardCharsets.US_ASCII));}
        return stream;
    }

    private static void embeddedPage(PDDocument document)throws IOException {
        try(InputStream stream=FontFixtures.class.getResourceAsStream(AppLocalFonts.RESOURCE)) {
            page(document,PDType0Font.load(document,stream),"Embedded \u00e9 \u03a9 \u0416");
        }
    }

    private static void page(PDDocument document,PDFont font,String text)throws IOException {
        PDPage page=new PDPage();document.addPage(page);
        try(PDPageContentStream content=new PDPageContentStream(document,page)) {
            content.beginText();content.setFont(font,12);content.newLineAtOffset(40,700);content.showText(text);content.endText();
        }
    }

    private static void markLastPage(PDDocument document,String name)throws IOException {
        PDPage page=document.getPage(document.getNumberOfPages()-1);
        for(boolean begin:new boolean[]{true,false}) {
            var mode=begin?PDPageContentStream.AppendMode.PREPEND:PDPageContentStream.AppendMode.APPEND;
            try(PDPageContentStream content=new PDPageContentStream(document,page,mode,false,true)) {
                content.beginText();content.setFont(new PDType1Font(FontName.HELVETICA),10);
                content.newLineAtOffset(40,begin?750:650);
                content.showText((begin?"BEGIN_":"END_")+name);content.endText();
            }
        }
    }

    private static void codes(PDDocument document,PDFont font,String hex)throws IOException {
        PDPage page=new PDPage();document.addPage(page);PDResources resources=new PDResources();page.setResources(resources);
        resources.put(COSName.getPDFName("F1"),font);
        var stream=new org.apache.pdfbox.pdmodel.common.PDStream(document);
        try(OutputStream output=stream.createOutputStream()) {
            output.write(("BT /F1 12 Tf 40 700 Td <"+hex+"> Tj ET\n").getBytes(StandardCharsets.US_ASCII));
        }
        page.setContents(stream);
    }

    private static byte[] save(PDDocument document)throws IOException {
        document.setDocumentId(0L);
        ByteArrayOutputStream output=new ByteArrayOutputStream();document.save(output);return output.toByteArray();
    }

    public static void main(String[] args)throws Exception {
        Files.write(Path.of(args[0]),corpus());
        Files.write(Path.of(args[1]),embedded());
    }
}
