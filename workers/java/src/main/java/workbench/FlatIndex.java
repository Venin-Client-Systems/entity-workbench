package workbench;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.channels.SeekableByteChannel;
import java.nio.file.*;
import java.nio.file.attribute.BasicFileAttributes;
import java.util.*;
import org.apache.lucene.index.DirectoryReader;
import org.apache.lucene.store.*;

/** Ordinary flat Lucene files, copied only through the coordinator's assigned directory. */
final class FlatIndex {
    static final String POLICY="lucene-10.5.1-bytebuffers-v1";
    static final String PROPERTY="workbench.directoryPolicy";
    private static final int BUFFER=64*1024;
    private FlatIndex() {}
    static boolean selected(String policy) throws IOException {
        if(policy==null) return false;
        if(!POLICY.equals(policy)) throw new IOException("Unknown directory policy");
        return true;
    }
    static void requireName(String name) throws IOException {
        if(name==null || !name.matches("[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}") || name.endsWith("."))
            throw new IOException("Invalid index filename");
        String stem=name.split("\\.",2)[0].toUpperCase(Locale.ROOT);
        if(Set.of("CON","PRN","AUX","NUL").contains(stem) || stem.matches("(?:COM|LPT)[1-9]"))
            throw new IOException("Reserved index filename");
    }
    private static SortedMap<String,Long> inventory(Path root) throws IOException {
        if(!Files.isDirectory(root,LinkOption.NOFOLLOW_LINKS)) throw new IOException("Invalid index directory");
        SortedMap<String,Long> entries=new TreeMap<>(); Set<String> names=new HashSet<>(); long total=0;
        try(var listing=Files.newDirectoryStream(root)) {
            for(Path file:listing) {
                if(entries.size()>=CappedDirectory.MAX_FILES) throw new IOException("Index member bound exceeded");
                String name=file.getFileName().toString(); requireName(name);
                if(!names.add(name.toLowerCase(Locale.ROOT))) throw new IOException("Index filename collision");
                BasicFileAttributes attr=Files.readAttributes(file,BasicFileAttributes.class,LinkOption.NOFOLLOW_LINKS);
                if(!attr.isRegularFile() || attr.size()<0 || attr.size()>CappedDirectory.MAX_FILE_BYTES)
                    throw new IOException("Invalid index member");
                total+=attr.size(); if(total>CappedDirectory.MAX_BYTES) throw new IOException("Index aggregate bound exceeded");
                entries.put(name,attr.size());
            }
        }
        return entries;
    }
    static CappedDirectory load(Path root) throws IOException {
        SortedMap<String,Long> files=inventory(root); CappedDirectory directory=new CappedDirectory();
        try {
            byte[] buffer=new byte[BUFFER];
            for(var file:files.entrySet()) {
                try(SeekableByteChannel input=Files.newByteChannel(root.resolve(file.getKey()),Set.of(StandardOpenOption.READ,LinkOption.NOFOLLOW_LINKS));
                    IndexOutput output=directory.createOutput(file.getKey(),IOContext.DEFAULT)) {
                    long remaining=file.getValue();
                    while(remaining>0) {
                        int read=input.read(ByteBuffer.wrap(buffer,0,(int)Math.min(buffer.length,remaining)));
                        if(read<=0) throw new IOException("Truncated index file");
                        output.writeBytes(buffer,0,read); remaining-=read;
                    }
                    if(input.read(ByteBuffer.wrap(buffer,0,1))!=-1) throw new IOException("Growing index file");
                }
            }
            if(!files.equals(inventory(root))) throw new IOException("Changed index inventory");
            directory.seal(); return directory;
        } catch(IOException | RuntimeException | Error failure) {
            try { directory.close(); } catch(IOException cleanup) { failure.addSuppressed(cleanup); }
            throw failure;
        }
    }
    static void requireRevision(String revision) throws IOException {
        if(revision==null || !revision.matches("0|[1-9][0-9]{0,19}")
                || new java.math.BigInteger(revision).bitLength()>64) throw new IOException("Invalid index revision");
    }
    static void verifyCommit(Directory directory,String revision) throws IOException {
        requireRevision(revision);
        try(var reader=DirectoryReader.open(directory)) {
            Map<String,String> expected=Map.of("workspace_revision",revision,"directory_policy",POLICY);
            if(!reader.getIndexCommit().getUserData().equals(expected)) throw new IOException("Index policy/revision mismatch");
        }
    }
    static void export(CappedDirectory directory,Path root,String revision) throws IOException {
        directory.seal(); verifyCommit(directory,revision);
        if(!inventory(root).isEmpty()) throw new IOException("Index export requires empty destination");
        byte[] buffer=new byte[BUFFER]; SortedMap<String,Long> exported=new TreeMap<>();
        for(String name:directory.listAll()) {
            requireName(name); long size=directory.fileLength(name); exported.put(name,size);
            try(IndexInput input=directory.openInput(name,IOContext.DEFAULT);
                SeekableByteChannel output=Files.newByteChannel(root.resolve(name),Set.of(StandardOpenOption.CREATE_NEW,StandardOpenOption.WRITE,LinkOption.NOFOLLOW_LINKS))) {
                long remaining=size;
                while(remaining>0) {
                    int count=(int)Math.min(buffer.length,remaining); input.readBytes(buffer,0,count);
                    ByteBuffer pending=ByteBuffer.wrap(buffer,0,count);
                    while(pending.hasRemaining()) if(output.write(pending)<=0) throw new IOException("Incomplete index export");
                    remaining-=count;
                }
                if(input.length()!=size || input.getFilePointer()!=size) throw new IOException("Changed index output");
            }
        }
        if(!exported.equals(inventory(root))) throw new IOException("Index export inventory mismatch");
        // Reopen the actual flat export before acknowledgement, not only its memory source.
        try(CappedDirectory check=load(root)) { verifyCommit(check,revision); }
    }
}
