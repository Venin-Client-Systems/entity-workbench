package workbench;

import com.fasterxml.jackson.databind.JsonNode;
import org.apache.tika.metadata.Metadata;
import org.apache.tika.parser.ParseContext;
import org.apache.tika.parser.microsoft.ooxml.OOXMLParser;
import org.apache.tika.extractor.EmbeddedDocumentExtractor;
import org.apache.tika.sax.BodyContentHandler;
import org.apache.tika.exception.WriteLimitReachedException;
import org.apache.pdfbox.Loader;
import org.apache.pdfbox.text.PDFTextStripper;
import org.apache.pdfbox.pdmodel.encryption.InvalidPasswordException;
import java.nio.file.*;
import java.nio.charset.*;
import java.nio.ByteBuffer;
import java.security.MessageDigest;
import java.util.*;
import java.util.zip.*;
import java.io.*;

/** One bounded document job per process. No Lucene classpath or canonical writes. */
public final class ParseWorker {
    private static final int TEXT_CHARS=128_000;
    private static final String DOCX="application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    private static final class TextLimit extends IOException {}
    private static final class ArchiveLimit extends IOException {}
    private static final class BoundedWriter extends Writer {
        private final StringBuilder text=new StringBuilder();
        public void write(char[] characters,int offset,int length)throws IOException {
            int remaining=TEXT_CHARS-text.length();
            text.append(characters,offset,Math.min(length,remaining));
            if(length>remaining)throw new TextLimit();
        }
        public void flush(){} public void close(){}
        public String toString(){return safePrefix(text.toString(),TEXT_CHARS);}
    }
    private static String safePrefix(String value,int limit) {
        int end=Math.min(value.length(),limit);
        if(end>0&&Character.isHighSurrogate(value.charAt(end-1)))end--;
        return value.substring(0,end);
    }
    private static Map<String,Object> result(JsonNode request,byte[] original)throws Exception {
        Map<String,Object> result=new LinkedHashMap<>();
        result.put("protocol_version",1);result.put("job_id",request.path("job_id").asText());
        result.put("content_sha256",HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(original)));
        result.put("source_bytes",original.length);result.put("parser","unsupported-v1");
        result.put("media_type","application/octet-stream");result.put("status","unsupported");
        result.put("text","");result.put("metadata",Map.of());
        result.put("limitations",new ArrayList<>(List.of("no_source_anchors")));result.put("error",null);
        return result;
    }
    @SuppressWarnings("unchecked")
    private static void limitation(Map<String,Object> result,String value) {
        List<String> limitations=(List<String>)result.get("limitations");
        if(!limitations.contains(value))limitations.add(value);
    }
    private static Map<String,List<String>> boundedMetadata(Metadata metadata,Map<String,Object> result) {
        Map<String,List<String>> fields=new TreeMap<>();int bytes=0;
        for(String name:new TreeSet<>(Arrays.asList(metadata.names()))) {
            if(fields.size()==32||name.getBytes(StandardCharsets.UTF_8).length>128||name.chars().anyMatch(Character::isISOControl)) {limitation(result,"metadata_limit");continue;}
            List<String> values=new ArrayList<>();bytes+=name.getBytes(StandardCharsets.UTF_8).length;
            for(String value:metadata.getValues(name)) {
                if(values.size()==8) {limitation(result,"metadata_limit");break;}
                // A conservative UTF-16 cap also bounds UTF-8 and JSON growth.
                String limited=safePrefix(value.replace("\0",""),1024);
                int size=limited.getBytes(StandardCharsets.UTF_8).length;
                if(limited.length()!=value.length())limitation(result,"metadata_limit");
                if(bytes+size>32_768) {limitation(result,"metadata_limit");break;}
                values.add(limited);bytes+=size;
            }
            if(bytes>32_768) {limitation(result,"metadata_limit");break;}
            fields.put(name,values);
        }
        return fields;
    }
    private static boolean inspectDocx(Path input,JsonNode request)throws IOException {
        int count=0;long total=0;boolean document=false,types=false;
        Set<String> names=new HashSet<>();
        try(ZipFile zip=new ZipFile(input.toFile())) {
            var entries=zip.entries();
            while(entries.hasMoreElements()) {
                var entry=entries.nextElement();String name=entry.getName();
                if(++count>request.path("limits").path("archive_members").asInt(0)
                    ||name.startsWith("/")||name.contains("\\")||name.contains(":")||!names.add(name))throw new ArchiveLimit();
                for(String part:name.split("/"))if(part.equals("..")||part.equals("."))throw new ArchiveLimit();
                if(entry.isDirectory())continue;
                if(entry.getSize()>16*1024*1024)throw new ArchiveLimit();
                long size=0;
                try(InputStream stream=zip.getInputStream(entry)) {
                    byte[] buffer=new byte[8192];int read;
                    while((read=stream.read(buffer))!=-1) {
                        size+=read;total+=read;
                        if(size>16*1024*1024||total>request.path("limits").path("expanded_bytes").asLong(0))throw new ArchiveLimit();
                    }
                }
                document|=name.equals("word/document.xml");types|=name.equals("[Content_Types].xml");
            }
        }
        return document&&types;
    }
    static void pdf(Path input,JsonNode request,Map<String,Object> result)throws Exception {
        boolean localFonts=AppLocalFonts.selected();
        result.put("parser",localFonts?AppLocalFonts.PARSER:"pdfbox-3.0.8");
        result.put("media_type","application/pdf");result.put("status","partial");
        limitation(result,"ocr_not_performed");limitation(result,"embedded_documents_excluded");
        try(AppLocalFonts fonts=localFonts?AppLocalFonts.install():null) {
            try {
                Protocol.checkpoint("pdf_load_started");
                try(var document=Loader.loadPDF(input.toFile())) {
                    Protocol.checkpoint("pdf_loaded");
                    if(!document.getCurrentAccessPermission().canExtractContent()) {fail(result,"text_extraction_restricted");return;}
                    int pages=request.path("limits").path("pages").asInt(0);
                    if(pages<1||pages>100)throw new IOException("Invalid page limit");
                    if(document.getNumberOfPages()>pages)limitation(result,"page_limit");
                    Protocol.checkpoint("pdf_stripper_started");
                    PDFTextStripper stripper=new PDFTextStripper();stripper.setEndPage(pages);
                    Protocol.checkpoint("pdf_stripper_ready");
                    BoundedWriter text=new BoundedWriter();
                    Protocol.checkpoint("pdf_text_started");
                    try {stripper.writeText(document,text);} catch(TextLimit limit) {limitation(result,"text_limit");}
                    Protocol.checkpoint("pdf_text_finished");
                    result.put("text",text.toString().replace("\0",""));
                    Metadata metadata=new Metadata();metadata.set("pdf:pages",Integer.toString(document.getNumberOfPages()));
                    var information=document.getDocumentInformation();
                    if(information.getTitle()!=null)metadata.set("title",information.getTitle());
                    if(information.getAuthor()!=null)metadata.set("author",information.getAuthor());
                    result.put("metadata",boundedMetadata(metadata,result));
                }
            } finally {
                if(fonts!=null&&fonts.used()) {
                    limitation(result,"font_substituted");
                    limitation(result,"font_coverage_unverified");
                }
            }
        }
    }
    private static void docx(Path input,Map<String,Object> result)throws Exception {
        result.put("parser","tika-ooxml-3.3.2");result.put("media_type",DOCX);result.put("status","partial");
        limitation(result,"embedded_documents_excluded");
        Metadata metadata=new Metadata();ParseContext context=new ParseContext();
        context.set(EmbeddedDocumentExtractor.class,new EmbeddedDocumentExtractor(){
            public boolean shouldParseEmbedded(Metadata value){return false;}
            public void parseEmbedded(InputStream stream,org.xml.sax.ContentHandler handler,Metadata value,boolean outputHtml){}
        });
        BodyContentHandler text=new BodyContentHandler(TEXT_CHARS);
        try(InputStream stream=Files.newInputStream(input)) {
            try {new OOXMLParser().parse(stream,text,metadata,context);}
            catch(Exception error) {if(WriteLimitReachedException.isWriteLimitReached(error))limitation(result,"text_limit");else throw error;}
        }
        result.put("text",safePrefix(text.toString(),TEXT_CHARS).replace("\0",""));
        result.put("metadata",boundedMetadata(metadata,result));
    }
    private static void fail(Map<String,Object> result,String error) {
        result.put("status","failed");result.put("error",error);result.put("text","");result.put("metadata",Map.of());
    }
    public static void main(String[] args) {
        try {
            JsonNode request=Protocol.read();
            if(!request.path("operation").asText().equals("parse")||request.path("inputs").size()!=1)throw new IOException("Unsupported parser request");
            Path input=Protocol.input(request.path("inputs").get(0).asText());byte[] original=Files.readAllBytes(input);
            Map<String,Object> result=result(request,original);
            try {
                if(original.length>=5&&new String(original,0,5,StandardCharsets.US_ASCII).equals("%PDF-"))pdf(input,request,result);
                else if(original.length>=4&&original[0]=='P'&&original[1]=='K'&&original[2]==3&&original[3]==4) {
                    result.put("media_type","application/zip");
                    result.put("parser","zip-preflight-v1");
                    if(inspectDocx(input,request))docx(input,result);
                    else result.put("parser","unsupported-v1");
                } else {
                    try {
                        String text=StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(original)).toString();
                        boolean ordinary=text.codePoints().noneMatch(c->Character.isISOControl(c)&&c!='\n'&&c!='\r'&&c!='\t'&&c!='\f');
                        if(ordinary) {
                            result.put("parser","utf8-v1");result.put("media_type","text/plain");result.put("status","complete");
                            result.put("text",safePrefix(text,TEXT_CHARS));
                            if(text.length()>TEXT_CHARS) {result.put("status","partial");limitation(result,"text_limit");}
                        }
                    } catch(CharacterCodingException unsupported) { /* retain explicit unsupported result */ }
                }
            } catch(AppLocalFonts.AssetException asset) {fail(result,"font_asset_unavailable");}
              catch(InvalidPasswordException encrypted) {fail(result,"encrypted_document");}
              catch(ArchiveLimit limit) {fail(result,"archive_limits");}
              catch(Exception malformed) {
                  fail(result,"malformed_document");
              }
            Protocol.write(request,result);
        } catch(Exception error) {Protocol.failure(error);}
    }
}
