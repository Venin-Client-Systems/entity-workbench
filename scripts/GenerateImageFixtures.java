import javax.imageio.ImageIO;
import java.awt.image.BufferedImage;
import java.nio.file.*;
import java.io.*;
import java.util.*;
import java.util.zip.CRC32;
/** Development-only fixture generation, never an application image decoder. */
public class GenerateImageFixtures {
    static byte[] chunk(String type,byte[] payload)throws Exception {
        var output=new ByteArrayOutputStream();var data=new DataOutputStream(output);
        data.writeInt(payload.length);byte[] name=type.getBytes(java.nio.charset.StandardCharsets.US_ASCII);data.write(name);data.write(payload);
        var crc=new CRC32();crc.update(name);crc.update(payload);data.writeInt((int)crc.getValue());return output.toByteArray();
    }
    static void insert(Path root,String name,byte[] image,byte[] extra)throws Exception {
        var output=new ByteArrayOutputStream();output.write(image,0,33);output.write(extra);output.write(image,33,image.length-33);Files.write(root.resolve(name),output.toByteArray());
    }
    static byte[] pngDimensions(byte[] png, int width, int height) {
        byte[] result=png.clone();
        for(int i=0;i<4;i++) {result[16+i]=(byte)(width>>>(24-8*i));result[20+i]=(byte)(height>>>(24-8*i));}
        var crc=new CRC32();crc.update(result,12,17);int value=(int)crc.getValue();
        for(int i=0;i<4;i++)result[29+i]=(byte)(value>>>(24-8*i));
        return result;
    }
    static byte[] jpegWidth(byte[] jpeg,int width) throws IOException {
        byte[] result=jpeg.clone();
        for(int offset=2;offset<result.length;) {
            if((result[offset]&255)!=255)throw new IOException("Unexpected synthetic JPEG structure");
            int marker=result[offset+1]&255;
            if(marker==0xc0) {result[offset+7]=(byte)(width>>>8);result[offset+8]=(byte)width;return result;}
            int length=((result[offset+2]&255)<<8)|(result[offset+3]&255);offset+=length+2;
        }
        throw new IOException("Synthetic JPEG frame header missing");
    }
    public static void main(String[] args)throws Exception {
        Path root=Path.of("fixtures/images");Files.createDirectories(root);
        byte[] pgm=Files.readAllBytes(Path.of("fixtures/ocr/synthetic.pgm"));int start=0;
        for(int i=0;i<3;i++){while(pgm[start++]!='\n'){} }
        BufferedImage image=new BufferedImage(1200,230,BufferedImage.TYPE_INT_RGB);
        for(int y=0;y<230;y++)for(int x=0;x<1200;x++){int gray=pgm[start+y*1200+x]&255;image.setRGB(x,y,(gray<<16)|(gray<<8)|gray);}
        ImageIO.write(image,"png",root.resolve("synthetic.png").toFile());ImageIO.write(image,"jpeg",root.resolve("synthetic.jpg").toFile());
        byte[] png=Files.readAllBytes(root.resolve("synthetic.png"));
        insert(root,"animated.png",png,chunk("acTL",new byte[]{0,0,0,2,0,0,0,0}));
        Files.write(root.resolve("oversize.png"),pngDimensions(png,8193,230));
        Files.write(root.resolve("pixel-limit.png"),pngDimensions(png,4000,4000));
        var extra=new ByteArrayOutputStream();for(int i=0;i<1024;i++)extra.write(chunk("tEXt",new byte[]{75,0,86}));
        insert(root,"chunk-limit.png",png,extra.toByteArray());
        byte[] malformed=png.clone();malformed[29]^=1;Files.write(root.resolve("bad-crc.png"),malformed);
        BufferedImage colors=new BufferedImage(2,2,BufferedImage.TYPE_INT_ARGB);colors.setRGB(0,0,0xffff0000);colors.setRGB(1,0,0xff00ff00);colors.setRGB(0,1,0xff0000ff);colors.setRGB(1,1,0x00000000);ImageIO.write(colors,"png",root.resolve("alpha.png").toFile());
        byte[] jpeg=Files.readAllBytes(root.resolve("synthetic.jpg"));var multiple=new ByteArrayOutputStream();multiple.write(jpeg);multiple.write(jpeg);Files.write(root.resolve("multiple.jpg"),multiple.toByteArray());
        Files.write(root.resolve("oversize.jpg"),jpegWidth(jpeg,8193));
        Files.write(root.resolve("truncated.jpg"),Arrays.copyOf(jpeg,jpeg.length-64));
        byte[] exif={69,120,105,102,0,0,73,73,42,0,8,0,0,0,1,0,18,1,3,0,1,0,0,0,6,0,0,0,0,0,0,0};
        var rotated=new ByteArrayOutputStream();var data=new DataOutputStream(rotated);data.write(jpeg,0,2);data.writeByte(255);data.writeByte(225);data.writeShort(exif.length+2);data.write(exif);data.write(jpeg,2,jpeg.length-2);Files.write(root.resolve("exif-orientation.jpg"),rotated.toByteArray());
        ImageIO.write(image,"gif",root.resolve("unsupported.gif").toFile());
    }
}
