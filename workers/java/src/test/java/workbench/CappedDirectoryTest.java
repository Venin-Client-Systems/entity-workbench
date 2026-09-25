package workbench;

import java.io.IOException;
import java.util.List;
import java.util.concurrent.*;
import org.apache.lucene.store.*;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class CappedDirectoryTest {
    @Test void exactFileAndAggregateLimitsReserveBeforeWritingAndPoisonOnOverflow() throws Exception {
        try(var directory=new CappedDirectory(3,4,6)) {
            try(var a=directory.createOutput("a",IOContext.DEFAULT); var b=directory.createOutput("b",IOContext.DEFAULT)) {
                a.writeBytes(new byte[4],0,4); b.writeBytes(new byte[2],0,2);
                assertEquals(6,directory.logicalBytes());
                assertThrows(IOException.class,()->b.writeByte((byte)1));
                assertEquals(2,b.getFilePointer()); assertEquals(6,directory.logicalBytes());
                assertThrows(IOException.class,()->directory.openInput("a",IOContext.DEFAULT));
            }
            assertThrows(IOException.class,directory::seal);
        }
        try(var directory=new CappedDirectory(2,4,8);var output=directory.createOutput("a",IOContext.DEFAULT)) {
            output.writeBytes(new byte[4],0,4);
            assertThrows(IOException.class,()->output.writeByte((byte)1));
            assertEquals(4,output.getFilePointer()); assertThrows(IOException.class,directory::listAll);
        }
    }
    @Test void productionFileAndMemberBoundsAreExact() throws Exception {
        try(var directory=new CappedDirectory()) {
            try(var output=directory.createOutput("large",IOContext.DEFAULT)) {
                byte[] block=new byte[64*1024];
                for(int i=0;i<128;i++) output.writeBytes(block,0,block.length);
                assertEquals(CappedDirectory.MAX_FILE_BYTES,output.getFilePointer());
                assertThrows(IOException.class,()->output.writeByte((byte)0));
                assertEquals(CappedDirectory.MAX_FILE_BYTES,output.getFilePointer());
            }
        }
        try(var directory=new CappedDirectory()) {
            for(int i=0;i<128;i++) try(var output=directory.createOutput("f"+i,IOContext.DEFAULT)) { output.writeByte((byte)i); }
            assertEquals(128,directory.listAll().length);
            assertThrows(IOException.class,()->directory.createOutput("overflow",IOContext.DEFAULT));
        }
    }
    @Test void productionAggregateBoundCountsAllLiveOutputsBeforeAllocation() throws Exception {
        try(var directory=new CappedDirectory()) {
            byte[] block=new byte[64*1024];
            for(int file=0;file<3;file++) try(var output=directory.createOutput("part"+file,IOContext.DEFAULT)) {
                for(int i=0;i<128;i++) output.writeBytes(block,0,block.length);
            }
            assertEquals(CappedDirectory.MAX_BYTES,directory.logicalBytes());
            try(var extra=directory.createOutput("extra",IOContext.DEFAULT)) {
                assertThrows(IOException.class,()->extra.writeByte((byte)0));assertEquals(0,extra.getFilePointer());
            }
            assertEquals(CappedDirectory.MAX_BYTES,directory.logicalBytes());assertThrows(IOException.class,directory::seal);
        }
    }
    @Test void renameDeleteAndOpenOutputAccountingRemainConsistent() throws Exception {
        try(var directory=new CappedDirectory(1,4,4)) {
            var output=directory.createOutput("a",IOContext.DEFAULT);output.writeBytes(new byte[4],0,4);
            assertThrows(IOException.class,()->directory.deleteFile("a"));
            assertThrows(IOException.class,()->directory.rename("a","b"));
            assertThrows(IOException.class,directory::seal);
            output.close();output.close();
            assertThrows(IOException.class,()->output.writeByte((byte)0));
            directory.rename("a","b");assertEquals(4,directory.fileLength("b"));
            assertThrows(IOException.class,()->directory.createOutput("B",IOContext.DEFAULT));
            directory.deleteFile("b");assertEquals(0,directory.logicalBytes());
            try(var next=directory.createOutput("next",IOContext.DEFAULT)){next.writeBytes(new byte[4],0,4);}
            assertEquals(4,directory.logicalBytes());
        }
    }
    @Test void sealedDirectoryRejectsEveryMutationAndCanRead() throws Exception {
        try(var directory=new CappedDirectory()) {
            try(var output=directory.createOutput("a",IOContext.DEFAULT)) {output.writeByte((byte)7);}
            directory.seal();
            try(var input=directory.openInput("a",IOContext.DEFAULT)){assertEquals(7,input.readByte());}
            assertThrows(IOException.class,()->directory.createOutput("b",IOContext.DEFAULT));
            assertThrows(IOException.class,()->directory.createTempOutput("a","b",IOContext.DEFAULT));
            assertThrows(IOException.class,()->directory.deleteFile("a"));
            assertThrows(IOException.class,()->directory.rename("a","b"));
            assertThrows(IOException.class,()->directory.obtainLock("write.lock"));
            assertThrows(IOException.class,()->directory.sync(List.of("a")));
            assertThrows(IOException.class,directory::syncMetaData);
            directory.close();assertThrows(IOException.class,directory::listAll);
        }
    }
    @Test void concurrentOutputsShareOneAggregateReservation() throws Exception {
        try(var directory=new CappedDirectory(2,4,6);var a=directory.createOutput("a",IOContext.DEFAULT);var b=directory.createOutput("b",IOContext.DEFAULT)) {
            CountDownLatch start=new CountDownLatch(1);
            try(var executor=Executors.newFixedThreadPool(2)) {
                var first=executor.submit(()->{start.await();try{a.writeBytes(new byte[4],0,4);return true;}catch(IOException denied){return false;}});
                var second=executor.submit(()->{start.await();try{b.writeBytes(new byte[4],0,4);return true;}catch(IOException denied){return false;}});
                start.countDown();assertNotEquals(first.get(5,TimeUnit.SECONDS),second.get(5,TimeUnit.SECONDS));
                assertEquals(4,directory.logicalBytes());assertThrows(IOException.class,directory::seal);
            }
        }
    }
    @Test void temporaryNamesAndPortableNamesAreControlled() throws Exception {
        try(var directory=new CappedDirectory()) {
            for(String name:List.of("../escape","a/b","a\\b","file:stream","CON.txt","LPT9",".hidden","a.","a ","é","x".repeat(129)))
                assertThrows(IOException.class,()->directory.createOutput(name,IOContext.DEFAULT),name);
            try(var first=directory.createTempOutput("_0","suffix",IOContext.DEFAULT);
                var second=directory.createTempOutput("_0","suffix",IOContext.DEFAULT)) {assertNotEquals(first.getName(),second.getName());}
        }
    }
}
