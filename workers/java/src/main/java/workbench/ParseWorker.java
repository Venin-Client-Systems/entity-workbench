package workbench;

import com.fasterxml.jackson.databind.JsonNode;
import org.apache.tika.metadata.Metadata;
import org.apache.tika.parser.*;
import org.apache.tika.extractor.EmbeddedDocumentExtractor;
import org.apache.tika.sax.BodyContentHandler;
import org.apache.pdfbox.Loader;
import org.apache.pdfbox.rendering.PDFRenderer;
import javax.imageio.ImageIO;
import java.nio.file.*;
import java.util.*;
import java.io.*;

/** One disposable document job per process. Never shares a process with Lucene. */
public final class ParseWorker {
    public static void main(String[] args) {
        try {
            JsonNode request=Protocol.read();
            String operation=request.path("operation").asText();
            Path input=Protocol.input(request.path("inputs").get(0).asText());
            if(operation.equals("parse")) {
                Metadata metadata=new Metadata();
                ParseContext context=new ParseContext();
                context.set(EmbeddedDocumentExtractor.class,new EmbeddedDocumentExtractor(){
                    public boolean shouldParseEmbedded(Metadata m){return false;}
                    public void parseEmbedded(InputStream in,org.xml.sax.ContentHandler handler,Metadata m,boolean outputHtml){}
                });
                int textLimit=(int)Math.min(1_000_000,request.path("limits").path("output_bytes").asLong(0)/2);
                if(textLimit<=0)throw new IOException("Invalid text limit");
                BodyContentHandler handler=new BodyContentHandler(textLimit);
                try(InputStream stream=Files.newInputStream(input)) {new AutoDetectParser().parse(stream,handler,metadata,context);}
                Map<String,String[]> fields=new TreeMap<>();
                for(String name:metadata.names())fields.put(name,metadata.getValues(name));
                Protocol.write(request,Map.of("text",handler.toString(),"metadata",fields,"status","partial","limitations",List.of("Embedded documents excluded","Page and cell anchors require dedicated extraction")));
            }else if(operation.equals("render_pdf")){
                try(var pdf=Loader.loadPDF(input.toFile())){
                    int pages=request.path("limits").path("pages").asInt(0);
                    if(pages<1||pdf.getNumberOfPages()>pages||pages>100)throw new IOException("Page limit exceeded");
                    PDFRenderer renderer=new PDFRenderer(pdf);List<String> images=new ArrayList<>();
                    long pixels=request.path("limits").path("pixels").asLong(0),used=0;
                    for(int i=0;i<pdf.getNumberOfPages();i++){
                        var box=pdf.getPage(i).getCropBox();double estimate=Math.ceil(box.getWidth()*1.5)*Math.ceil(box.getHeight()*1.5);
                        if(!Double.isFinite(estimate)||estimate<1||estimate>pixels-used)throw new IOException("Pixel limit exceeded");
                        var image=renderer.renderImageWithDPI(i,108);used+=(long)image.getWidth()*image.getHeight();
                        String name="page-"+(i+1)+".png";ImageIO.write(image,"png",Protocol.relative(name).toFile());images.add(name);
                    }
                    Protocol.write(request,Map.of("pages",images,"status","complete"));
                }
            }else throw new IOException("Unsupported parsing operation");
        }catch(Exception exception){Protocol.failure(exception);}
    }
}
