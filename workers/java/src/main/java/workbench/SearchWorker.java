package workbench;

import com.fasterxml.jackson.databind.JsonNode;
import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.store.FSDirectory;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.queryparser.classic.QueryParser;
import java.nio.file.*;
import java.util.*;
import java.io.*;

/** A distinct process and data directory, never loaded by the document parser. */
public final class SearchWorker {
    public static void main(String[] args){
        try{
            JsonNode request=Protocol.read();String operation=request.path("operation").asText();
            Protocol.checkpoint("search_request_validated");
            // The Rust coordinator assigns the only index allowed by the OS profile.
            String assignedIndex=System.getProperty("workbench.index");
            Path index=assignedIndex==null ? Protocol.relative("index") : Path.of(assignedIndex);
            if(Files.isSymbolicLink(index)||!Files.isDirectory(index,LinkOption.NOFOLLOW_LINKS)) {
                if(assignedIndex!=null)throw new IOException("Invalid assigned index");
            }
            Protocol.checkpoint("search_index_validated");
            Protocol.checkpoint("search_directory_started");
            try(var directory=FSDirectory.open(index);var analyzer=new StandardAnalyzer()){
                Protocol.checkpoint("search_directory_ready");
                if(operation.equals("index")){
                    JsonNode manifest=Protocol.JSON.readTree(Files.readAllBytes(Protocol.input(request.path("inputs").get(0).asText())));
                    Protocol.checkpoint("search_manifest_read");
                    if(!manifest.path("documents").isArray()||manifest.path("documents").size()>10000)throw new IOException("Document count limit");
                    Protocol.checkpoint("search_writer_started");
                    try(var writer=new IndexWriter(directory,new IndexWriterConfig(analyzer).setOpenMode(IndexWriterConfig.OpenMode.CREATE))){
                        Protocol.checkpoint("search_writer_ready");
                        for(JsonNode source:manifest.path("documents")){
                            Document doc=new Document();
                            doc.add(new StringField("id",source.path("id").asText(),Field.Store.YES));
                            doc.add(new TextField("text",source.path("text").asText(),Field.Store.NO));
                            doc.add(new TextField("name",source.path("name").asText(),Field.Store.YES));
                            writer.addDocument(doc);
                        }
                        writer.setLiveCommitData(Map.of("workspace_revision",manifest.path("workspace_revision").asText()).entrySet());
                    }
                    Protocol.checkpoint("search_index_committed");
                    Protocol.write(request,Map.of("indexed",manifest.path("documents").size(),"workspace_revision",manifest.path("workspace_revision").asLong()));
                }else if(operation.equals("search")){
                    JsonNode search=Protocol.JSON.readTree(Files.readAllBytes(Protocol.input(request.path("inputs").get(0).asText())));
                    String query=search.path("query").asText();if(query.isBlank()||query.length()>1024)throw new IOException("Invalid query");
                    try(var reader=DirectoryReader.open(directory)){
                        IndexSearcher engine=new IndexSearcher(reader);QueryParser parser=new QueryParser("text",analyzer);parser.setAllowLeadingWildcard(false);
                        var hits=engine.search(parser.parse(query),100);List<Map<String,Object>> results=new ArrayList<>();
                        for(var hit:hits.scoreDocs){var doc=engine.storedFields().document(hit.doc);results.add(Map.of("id",doc.get("id"),"name",doc.get("name"),"score",hit.score));}
                        Protocol.write(request,Map.of("workspace_revision",reader.getIndexCommit().getUserData().get("workspace_revision"),"hits",results,"total",hits.totalHits.value()));
                    }
                }else throw new IOException("Unsupported search operation");
            }
        }catch(Exception exception){Protocol.failure(exception);}
    }
}
