package workbench;

import java.io.IOException;
import java.nio.channels.FileChannel;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.TimeUnit;
import com.fasterxml.jackson.databind.JsonNode;
import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.queryparser.classic.QueryParser;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.store.*;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import static org.junit.jupiter.api.Assertions.*;

class FlatIndexTest {
    @TempDir Path root;
    static void writeIndex(Directory directory,Map<String,String> metadata) throws Exception {
        try(var analyzer=new StandardAnalyzer();var writer=new IndexWriter(directory,new IndexWriterConfig(analyzer))) {
            var document=new Document();document.add(new StringField("id","synthetic",Field.Store.YES));
            document.add(new TextField("name","notice.txt",Field.Store.YES));
            document.add(new TextField("text","Aster Harbour synthetic notice",Field.Store.NO));writer.addDocument(document);
            writer.setLiveCommitData(metadata.entrySet());
        }
    }
    static Map<String,String> metadata() {return Map.of("workspace_revision","7","directory_policy",FlatIndex.POLICY);}
    static void queries(Directory directory) throws Exception {
        try(var reader=DirectoryReader.open(directory);var analyzer=new StandardAnalyzer()) {
            var searcher=new IndexSearcher(reader);var parser=new QueryParser("text",analyzer);parser.setAllowLeadingWildcard(false);
            for(String query:List.of("Aster AND Harbour","\"Aster Harbour\"","\"Harbour Aster\"~2","Astre~1","name:notice.txt"))
                assertEquals(1,searcher.search(parser.parse(query),10).totalHits.value(),query);
            assertEquals(0,searcher.search(parser.parse("unmatched"),10).totalHits.value());
        }
    }
    @Test void ordinarySegmentsInteroperateBothWaysAtPinnedLucene() throws Exception {
        assertEquals("10.5.1",org.apache.lucene.util.Version.LATEST.toString());
        Path filesystem=Files.createDirectory(root.resolve("filesystem"));
        try(var directory=FSDirectory.open(filesystem)) {writeIndex(directory,metadata());}
        try(var imported=FlatIndex.load(filesystem)) {FlatIndex.verifyCommit(imported,"7");queries(imported);}
        Path exported=Files.createDirectory(root.resolve("exported"));
        try(var memory=new CappedDirectory()) {writeIndex(memory,metadata());FlatIndex.export(memory,exported,"7");}
        try(var directory=FSDirectory.open(exported)) {FlatIndex.verifyCommit(directory,"7");queries(directory);}
    }
    @Test void missingWrongPolicyRevisionAndCorruptCommitReject() throws Exception {
        for(Map<String,String> data:List.of(Map.of("workspace_revision","7"),Map.of("workspace_revision","7","directory_policy","other"),
                Map.of("workspace_revision","8","directory_policy",FlatIndex.POLICY),Map.of("workspace_revision","7","directory_policy",FlatIndex.POLICY,"extra","value"))) {
            try(var directory=new CappedDirectory()) {writeIndex(directory,data);assertThrows(IOException.class,()->FlatIndex.verifyCommit(directory,"7"));}
        }
        Path files=Files.createDirectory(root.resolve("index"));
        try(var memory=new CappedDirectory()) {writeIndex(memory,metadata());FlatIndex.export(memory,files,"7");}
        Path commit;
        try(var paths=Files.list(files)) {commit=paths.filter(p->p.getFileName().toString().startsWith("segments_")).findFirst().orElseThrow();}
        Files.write(commit,new byte[]{0});
        try(var imported=FlatIndex.load(files)) {assertThrows(IOException.class,()->FlatIndex.verifyCommit(imported,"7"));}
        Files.delete(commit);
        try(var imported=FlatIndex.load(files)) {assertThrows(IOException.class,()->FlatIndex.verifyCommit(imported,"7"));}
        for(String bad:List.of("-1","01","18446744073709551616","1.0"," ")) assertThrows(IOException.class,()->FlatIndex.requireRevision(bad));
        FlatIndex.requireRevision("18446744073709551615");
    }
    @Test void exportNeverOverwritesAnExistingDestination() throws Exception {
        Path files=Files.createDirectory(root.resolve("existing"));Files.writeString(files.resolve("sentinel"),"unchanged");
        try(var memory=new CappedDirectory()) {writeIndex(memory,metadata());assertThrows(IOException.class,()->FlatIndex.export(memory,files,"7"));}
        assertEquals("unchanged",Files.readString(files.resolve("sentinel")));
        try(var paths=Files.list(files)) {assertEquals(List.of("sentinel"),paths.map(p->p.getFileName().toString()).toList());}
    }
    @Test void importRejectsNonflatUnsafeOversizedAndExcessMembers() throws Exception {
        Path files=Files.createDirectory(root.resolve("unsafe"));Files.createDirectory(files.resolve("nested"));
        assertThrows(IOException.class,()->FlatIndex.load(files));Files.delete(files.resolve("nested"));
        Files.writeString(files.resolve(".hidden"),"x");assertThrows(IOException.class,()->FlatIndex.load(files));Files.delete(files.resolve(".hidden"));
        // Sparse allocation keeps a bound regression small without constructing oversized arrays.
        try(var channel=FileChannel.open(files.resolve("large"),StandardOpenOption.CREATE_NEW,StandardOpenOption.WRITE)) {
            channel.position(CappedDirectory.MAX_FILE_BYTES);channel.write(java.nio.ByteBuffer.wrap(new byte[1]));
        }
        assertThrows(IOException.class,()->FlatIndex.load(files));Files.delete(files.resolve("large"));
        for(int i=0;i<129;i++) Files.createFile(files.resolve("f"+i));
        assertThrows(IOException.class,()->FlatIndex.load(files));
    }
    @Test void importExactAggregateAndMemberLimitsRejectPlusOne() throws Exception {
        Path files=Files.createDirectory(root.resolve("aggregate"));
        for(int i=0;i<3;i++) try(var channel=FileChannel.open(files.resolve("part"+i),StandardOpenOption.CREATE_NEW,StandardOpenOption.WRITE)) {
            channel.position(CappedDirectory.MAX_FILE_BYTES-1);channel.write(java.nio.ByteBuffer.wrap(new byte[1]));
        }
        try(var imported=FlatIndex.load(files)) {assertEquals(CappedDirectory.MAX_BYTES,imported.logicalBytes());}
        Files.write(files.resolve("extra"),new byte[1]);assertThrows(IOException.class,()->FlatIndex.load(files));
        Path members=Files.createDirectory(root.resolve("members"));
        for(int i=0;i<128;i++)Files.createFile(members.resolve("f"+i));
        try(var imported=FlatIndex.load(members)) {assertEquals(128,imported.listAll().length);}
        Files.createFile(members.resolve("extra"));assertThrows(IOException.class,()->FlatIndex.load(members));
    }
    private JsonNode child(String label,String policy,String operation,Path index,Object input,boolean success) throws Exception {
        Path scratch=Files.createDirectory(root.resolve(label));Path data=scratch.resolve("input.json");
        Files.write(data,Protocol.JSON.writeValueAsBytes(input));
        Path request=scratch.resolve("request.json");
        Files.write(request,Protocol.JSON.writeValueAsBytes(Map.of("protocol_version",1,"job_id","00000000-0000-4000-8000-000000000001",
            "operation",operation,"inputs",List.of("input.json"),"output","result.json","limits",Map.of("output_bytes",1024*1024))));
        String java=Path.of(System.getProperty("java.home"),"bin",System.getProperty("os.name").startsWith("Windows")?"java.exe":"java").toString();
        var args=new ArrayList<>(List.of(java,"-Xmx256m","-Dworkbench.index="+index,"-Dworkbench.assignedInput="+data));
        if(policy!=null)args.add("-D"+FlatIndex.PROPERTY+"="+policy);
        args.addAll(List.of("-cp",System.getProperty("surefire.test.class.path",System.getProperty("java.class.path")),"workbench.FileWorker",operation,request.toString()));
        Process process=new ProcessBuilder(args).directory(scratch.toFile()).redirectOutput(ProcessBuilder.Redirect.DISCARD).redirectError(ProcessBuilder.Redirect.DISCARD).start();
        try {assertTrue(process.waitFor(30,TimeUnit.SECONDS));assertEquals(success?0:1,process.exitValue());}
        finally {if(process.isAlive()){process.destroyForcibly();assertTrue(process.waitFor(5,TimeUnit.SECONDS));}}
        Path result=scratch.resolve("result.json");
        if(!success) {assertFalse(Files.exists(result));return null;}
        byte[] output;try(var stream=Files.newInputStream(result)){output=stream.readNBytes(1024*1024+1);}
        assertTrue(output.length<=1024*1024);return Protocol.JSON.readTree(output);
    }
    @Test void explicitRecipeBindsRepliesAndPropertyAbsentRetainsFilesystemContract() throws Exception {
        Object manifest=Map.of("workspace_revision",7,"documents",List.of(Map.of("id","synthetic","name","notice.txt","text","Aster Harbour")));
        for(String mode:List.of("memory","default")) {
            String policy=mode.equals("memory")?FlatIndex.POLICY:null;Path index=Files.createDirectory(root.resolve(mode));
            JsonNode ack=child(mode+"-index",policy,"index",index,manifest,true);
            assertEquals(1,ack.path("indexed").asInt());assertEquals(7,ack.path("workspace_revision").asInt());
            assertEquals(policy!=null,ack.has("directory_policy"));if(policy!=null)assertEquals(policy,ack.path("directory_policy").asText());
            Object query=policy==null?Map.of("query","Aster"):Map.of("query","Aster","workspace_revision","7","directory_policy",policy);
            JsonNode hits=child(mode+"-search",policy,"search",index,query,true);
            assertEquals("7",hits.path("workspace_revision").asText());assertEquals(1,hits.path("total").asInt());
            assertEquals("synthetic",hits.path("hits").get(0).path("id").asText());assertEquals(policy!=null,hits.has("directory_policy"));
            try(var directory=FSDirectory.open(index);var reader=DirectoryReader.open(directory)) {
                assertEquals(policy==null?Map.of("workspace_revision","7"):metadata(),reader.getIndexCommit().getUserData());
            }
            if(policy!=null) child("missing-query-policy",policy,"search",index,Map.of("query","Aster"),false);
        }
        Path unknown=Files.createDirectory(root.resolve("unknown"));child("unknown-policy","other","index",unknown,manifest,false);
        try(var paths=Files.list(unknown)) {assertEquals(0,paths.count());}
        assertFalse(FlatIndex.selected(null));assertTrue(FlatIndex.selected(FlatIndex.POLICY));assertThrows(IOException.class,()->FlatIndex.selected(""));
    }
}
