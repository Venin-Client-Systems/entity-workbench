package workbench;

import java.io.IOException;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.NoSuchFileException;
import java.util.*;
import org.apache.lucene.store.*;

/** Per-job logical-byte limits. Heap overhead remains bounded by the JVM/Job limits. */
final class CappedDirectory extends Directory {
    static final int MAX_FILES = 128;
    static final long MAX_FILE_BYTES = 8L * 1024 * 1024;
    static final long MAX_BYTES = 24L * 1024 * 1024;
    private final Object guard = new Object();
    private final ByteBuffersDirectory storage = new ByteBuffersDirectory();
    private final Map<String, Entry> files = new HashMap<>();
    private final int maxFiles;
    private final long maxFileBytes, maxBytes;
    private long bytes, temporary;
    private boolean sealed, closed, poisoned;

    private static final class Entry {
        long bytes;
        boolean open = true;
    }
    CappedDirectory() { this(MAX_FILES, MAX_FILE_BYTES, MAX_BYTES); }
    // Smaller caps make exact-edge tests cheap; production always uses the fixed constructor.
    CappedDirectory(int maxFiles, long maxFileBytes, long maxBytes) {
        if(maxFiles < 1 || maxFiles > MAX_FILES || maxFileBytes < 1 || maxFileBytes > MAX_FILE_BYTES
                || maxBytes < 1 || maxBytes > MAX_BYTES) throw new IllegalArgumentException("Invalid directory limits");
        this.maxFiles=maxFiles; this.maxFileBytes=maxFileBytes; this.maxBytes=maxBytes;
    }
    private void readable() throws IOException {
        if(closed || poisoned) throw new IOException("Directory unavailable");
    }
    private void mutable() throws IOException {
        readable(); if(sealed) throw new IOException("Read-only directory");
    }
    private Entry entry(String name) throws IOException {
        Entry entry=files.get(name);
        if(entry==null) throw new NoSuchFileException("Index member absent");
        return entry;
    }
    private void unused(String name) throws IOException {
        FlatIndex.requireName(name);
        for(String present:files.keySet()) if(present.equalsIgnoreCase(name))
            throw new FileAlreadyExistsException("Index member already exists");
        if(files.size()>=maxFiles) throw new IOException("Index member bound exceeded");
    }
    void seal() throws IOException {
        synchronized(guard) {
            readable();
            if(files.values().stream().anyMatch(e->e.open)) throw new IOException("Open index output");
            sealed=true;
        }
    }
    long logicalBytes() { synchronized(guard) { return bytes; } }
    @Override public String[] listAll() throws IOException {
        synchronized(guard) { readable(); return files.keySet().stream().sorted().toArray(String[]::new); }
    }
    @Override public long fileLength(String name) throws IOException {
        synchronized(guard) { readable(); return entry(name).bytes; }
    }
    @Override public IndexOutput createOutput(String name, IOContext context) throws IOException {
        synchronized(guard) {
            mutable(); unused(name);
            IndexOutput output=storage.createOutput(name,context);
            Entry entry=new Entry(); files.put(name,entry);
            return new CappedOutput(name,output,entry);
        }
    }
    @Override public IndexOutput createTempOutput(String prefix,String suffix,IOContext context) throws IOException {
        synchronized(guard) {
            mutable();
            for(int tries=0;tries<=maxFiles;tries++) {
                String name=getTempFileName(prefix,suffix,temporary++);
                if(files.keySet().stream().noneMatch(n->n.equalsIgnoreCase(name))) return createOutput(name,context);
            }
            throw new IOException("Temporary index name exhausted");
        }
    }
    @Override public void deleteFile(String name) throws IOException {
        synchronized(guard) {
            mutable(); Entry entry=entry(name);
            if(entry.open) throw new IOException("Cannot delete open index output");
            storage.deleteFile(name); files.remove(name); bytes-=entry.bytes;
        }
    }
    @Override public void rename(String source,String destination) throws IOException {
        synchronized(guard) {
            mutable(); Entry entry=entry(source);
            if(entry.open) throw new IOException("Cannot rename open index output");
            FlatIndex.requireName(destination);
            for(String present:files.keySet()) if(present.equalsIgnoreCase(destination))
                throw new FileAlreadyExistsException("Index rename collision");
            storage.rename(source,destination); files.remove(source); files.put(destination,entry);
        }
    }
    @Override public IndexInput openInput(String name,IOContext context) throws IOException {
        synchronized(guard) {
            readable(); if(entry(name).open) throw new IOException("Index output is open");
            return storage.openInput(name,context);
        }
    }
    @Override public void sync(Collection<String> names) throws IOException {
        synchronized(guard) { mutable(); for(String name:names) if(entry(name).open) throw new IOException("Open index output"); storage.sync(names); }
    }
    @Override public void syncMetaData() throws IOException { synchronized(guard) { mutable(); storage.syncMetaData(); } }
    @Override public Lock obtainLock(String name) throws IOException {
        synchronized(guard) { mutable(); FlatIndex.requireName(name); return storage.obtainLock(name); }
    }
    @Override public Set<String> getPendingDeletions() throws IOException { synchronized(guard) { readable(); return Set.of(); } }
    @Override public void close() throws IOException {
        synchronized(guard) { if(!closed) { closed=true; storage.close(); files.clear(); bytes=0; } }
    }
    private final class CappedOutput extends IndexOutput {
        private final IndexOutput output;
        private final Entry entry;
        private boolean outputClosed;
        CappedOutput(String name,IndexOutput output,Entry entry) {
            super("bounded memory index",name); this.output=output; this.entry=entry;
        }
        private void reserve(int length) throws IOException {
            mutable();
            if(outputClosed) throw new IOException("Index output closed");
            // Reserve before any ByteBuffersDataOutput allocation, including concurrent merge outputs.
            if(length<0 || length>maxFileBytes-entry.bytes || length>maxBytes-bytes) {
                poisoned=true; throw new IOException("Index logical byte bound exceeded");
            }
            entry.bytes+=length; bytes+=length;
        }
        @Override public void writeByte(byte value) throws IOException {
            synchronized(guard) {
                reserve(1);
                try { output.writeByte(value); } catch(IOException | RuntimeException | Error failure) { poisoned=true; throw failure; }
            }
        }
        @Override public void writeBytes(byte[] source,int offset,int length) throws IOException {
            Objects.checkFromIndexSize(offset,length,source.length);
            synchronized(guard) {
                reserve(length);
                try { output.writeBytes(source,offset,length); } catch(IOException | RuntimeException | Error failure) { poisoned=true; throw failure; }
            }
        }
        @Override public long getFilePointer() { synchronized(guard) { return entry.bytes; } }
        @Override public long getChecksum() throws IOException {
            synchronized(guard) { readable(); if(outputClosed) throw new IOException("Index output closed"); return output.getChecksum(); }
        }
        @Override public void close() throws IOException {
            synchronized(guard) {
                if(outputClosed) return;
                outputClosed=true;
                try { output.close(); entry.open=false; }
                catch(IOException | RuntimeException | Error failure) { poisoned=true; throw failure; }
            }
        }
    }
}
