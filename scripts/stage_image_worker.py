"""Compile the image adapter with an installed JDK, and stage only its required local assets."""
import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile
ROOT = Path(__file__).resolve().parents[1]


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--javac",type=Path,required=True)
    parser.add_argument("--java-runtime",type=Path,required=True)
    parser.add_argument("--dependencies",type=Path,required=True)
    parser.add_argument("--runtime",type=Path,default=ROOT/"runtime/staged/engines")
    args=parser.parse_args()
    if (args.runtime/"image").exists(): parser.error("Use a fresh image staging destination")
    jars=[args.dependencies/name for name in ("jackson-annotations-2.22.jar","jackson-core-2.22.3.jar","jackson-databind-2.22.3.jar")]
    if not all(p.is_file() for p in jars): parser.error("Required cached Jackson JARs are missing")
    java=args.java_runtime.resolve(strict=True)
    if not (java/"bin/java").is_file(): parser.error("Existing Java runtime required")
    with tempfile.TemporaryDirectory() as temporary:
        classes=Path(temporary)/"classes";classes.mkdir()
        sources=[ROOT/"workers/java/src/main/java/workbench"/name for name in ("Protocol.java","ImageWorker.java","HostileProbe.java")]
        subprocess.run([str(args.javac),"--release","21","-cp",":".join(map(str,jars)),"-d",str(classes),*map(str,sources)],check=True)
        if not (args.runtime/"java").exists(): shutil.copytree(java,args.runtime/"java")
        target=args.runtime/"image"; (target/"lib").mkdir(parents=True)
        with zipfile.ZipFile(target/"workers-0.1.0.jar","w",compression=zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(classes.rglob("*.class")): archive.write(path,str(path.relative_to(classes)))
        for path in jars: shutil.copyfile(path,target/"lib"/path.name)
    print("Staged separate image classpath with JDK readers and three cached Jackson JARs; no downloads")


if __name__=="__main__": main()
