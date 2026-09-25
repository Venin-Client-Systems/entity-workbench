//! App-owned fixed-name bundle I/O; hashes are integrity checks, not authenticity signatures.
use super::*;
use std::{fs::File, io::Read};

struct Node {
    file: File,
    directory: bool,
}
fn open(path: &Path, directory: bool) -> Result<File> {
    reject_link_ancestors(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 })
            .share_mode(1);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    require(
        !is_link(&meta)
            && if directory {
                meta.is_dir()
            } else {
                meta.is_file()
            },
        "Invalid collection bundle node",
    )?;
    links(&file, directory)?;
    Ok(file)
}
fn links(file: &File, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        require(
            directory || file.metadata()?.nlink() == 1,
            "Hard-linked bundle file refused",
        )?;
    }
    #[cfg(windows)]
    {
        let _ = directory;
        super::super::super::file_identity::windows_handle(file, 1)?;
    }
    Ok(())
}
fn identity(a: &File, b: &File) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let a = a.metadata()?;
        let b = b.metadata()?;
        require(
            a.dev() == b.dev() && a.ino() == b.ino(),
            "Collection bundle node was replaced",
        )?;
    }
    #[cfg(windows)]
    {
        require(
            super::super::super::file_identity::windows_handle(a, 1)?
                == super::super::super::file_identity::windows_handle(b, 1)?,
            "Collection bundle node was replaced",
        )?;
    }
    Ok(())
}
impl Node {
    fn open(path: &Path, directory: bool) -> Result<Self> {
        Ok(Self {
            file: open(path, directory)?,
            directory,
        })
    }
    fn check(&self, path: &Path) -> Result<()> {
        let named = open(path, self.directory)?;
        identity(&self.file, &named)
    }
}
pub(super) struct Bundle {
    root: PathBuf,
    directory: Node,
    originals: Node,
}
impl Bundle {
    pub(super) fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            root: root.into(),
            directory: Node::open(root, true)?,
            originals: Node::open(&root.join("originals"), true)?,
        })
    }
    fn check(&self) -> Result<()> {
        self.directory.check(&self.root)?;
        self.originals.check(&self.root.join("originals"))
    }
    pub(super) fn read(&self, name: &str, cap: usize) -> Result<Vec<u8>> {
        self.check()?;
        let path = self.root.join(name);
        let mut file = open(&path, false)?;
        let before = file.metadata()?;
        require(before.len() <= cap as u64, "Bundle file exceeds read bound")?;
        let mut bytes = Vec::with_capacity(before.len() as usize);
        (&mut file).take(cap as u64 + 1).read_to_end(&mut bytes)?;
        require(
            bytes.len() == before.len() as usize && bytes.len() <= cap,
            "Bundle file changed length",
        )?;
        super::super::super::file_identity::unchanged(&before, &file.metadata()?)?;
        let named = open(&path, false)?;
        identity(&file, &named)?;
        super::super::super::file_identity::unchanged(&before, &named.metadata()?)?;
        self.check()?;
        Ok(bytes)
    }
    pub(super) fn inventory(&self, files: &[String]) -> Result<()> {
        self.check()?;
        let expected: BTreeSet<_> = files.iter().cloned().collect();
        require(expected.len() == files.len(), "Duplicate bundle inventory")?;
        let mut actual = BTreeSet::new();
        for entry in fs::read_dir(&self.root)?.take(5) {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Validation("Invalid bundle entry".into()))?;
            if name == "originals" {
                continue;
            }
            require(actual.len() < 2, "Unexpected bundle entry")?;
            Node::open(&entry.path(), false)?;
            actual.insert(name);
        }
        for entry in fs::read_dir(self.root.join("originals"))?.take(51) {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Validation("Invalid original bundle entry".into()))?;
            Node::open(&entry.path(), false)?;
            actual.insert(format!("originals/{name}"));
            require(actual.len() <= 52, "Bundle original count exceeds bound")?;
        }
        require(actual == expected, "Bundle file inventory differs")?;
        self.check()
    }
}
pub(super) struct OwnedBundle {
    root: PathBuf,
    directory: Node,
    directories: Vec<(String, Node)>,
    files: Vec<(String, Node, String, usize)>,
}
impl OwnedBundle {
    pub(super) fn create(root: &Path) -> Result<Self> {
        Self::create_registered(root, |_| Ok(()))
    }
    fn create_registered(root: &Path, register: impl FnOnce(&Path) -> Result<()>) -> Result<Self> {
        reject_link_ancestors(root)?;
        create_directory(root)?; // Existing complete OR partial IDs are never adopted.
        let directory = register(root).and_then(|()| Node::open(root, true)).map_err(|_| {
            Error::Cleanup("Collection export directory was created but ownership could not be verified; retained partial export identifier must not be reused".into())
        })?;
        Ok(Self {
            root: root.into(),
            directory,
            directories: vec![],
            files: vec![],
        })
    }
    fn check(&self) -> Result<()> {
        self.directory.check(&self.root)?;
        for (name, node) in &self.directories {
            node.check(&self.root.join(name))?;
        }
        Ok(())
    }
    pub(super) fn directory(&mut self, name: &str) -> Result<()> {
        self.directory_registered(name, |_| Ok(()))
    }
    fn directory_registered(
        &mut self,
        name: &str,
        register: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<()> {
        self.check()?;
        let path = self.root.join(name);
        create_directory(&path)?;
        let node = register(&path).and_then(|()| Node::open(&path, true)).map_err(|_| {
            Error::Cleanup("Collection export child directory was created but ownership could not be verified; retained partial export identifier must not be reused".into())
        })?;
        self.directories.push((name.into(), node));
        Ok(())
    }
    pub(super) fn write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.check()?;
        let path = self.root.join(name);
        reject_link_ancestors(&path)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        // A failed write before registration leaves an unrecognized entry. Cleanup
        // refuses a nonempty directory instead of deleting an unverified path.
        file.write_all(bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o400))?;
        }
        file.sync_all()?;
        let before = file.metadata()?;
        #[cfg(windows)]
        let created_identity = super::super::super::file_identity::windows_handle(&file, 1)?;
        drop(file);
        let node = Node::open(&path, false)?;
        #[cfg(windows)]
        require(
            created_identity == super::super::super::file_identity::windows_handle(&node.file, 1)?,
            "Written bundle file was replaced",
        )?;
        super::super::super::file_identity::unchanged(&before, &node.file.metadata()?)?;
        self.files
            .push((name.into(), node, hash(bytes), bytes.len()));
        Ok(())
    }
    pub(super) fn verify(&self) -> Result<()> {
        self.check()?;
        let bundle = Bundle::open(&self.root)?;
        for (name, node, digest, bytes) in &self.files {
            node.check(&self.root.join(name))?;
            let raw = bundle.read(name, *bytes)?;
            require(
                raw.len() == *bytes && hash(&raw) == *digest,
                "Owned export bytes changed",
            )?;
        }
        bundle.inventory(
            &self
                .files
                .iter()
                .map(|(name, _, _, _)| name.clone())
                .collect::<Vec<_>>(),
        )
    }
    pub(super) fn sync(&self) -> Result<()> {
        self.check()?;
        #[cfg(unix)]
        {
            for (_, node) in &self.directories {
                node.file.sync_all()?;
            }
            self.directory.file.sync_all()?;
        }
        Ok(())
    }
    pub(super) fn cleanup(mut self) -> Result<()> {
        self.check()?;
        // Never recurse into unexpected, linked, modified or replaced contents.
        self.verify()?;
        for (name, node, _, _) in self.files.drain(..).rev() {
            let path = self.root.join(name);
            node.check(&path)?;
            drop(node);
            fs::remove_file(path)?;
        }
        for (name, node) in self.directories.drain(..).rev() {
            let path = self.root.join(name);
            node.check(&path)?;
            drop(node);
            fs::remove_dir(path)?;
        }
        self.directory.check(&self.root)?;
        drop(self.directory);
        fs::remove_dir(&self.root)?;
        Ok(())
    }
}

fn create_directory(path: &Path) -> Result<()> {
    reject_link_ancestors(path)?;
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_create_registration_failure_retains_partial_without_unproven_cleanup() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let root = temp.path().join("bundle");
        let result = OwnedBundle::create_registered(&root, |path| {
            fs::write(path.join("unregistered"), b"retained")?;
            Err(Error::Validation("injected registration failure".into()))
        });
        assert!(matches!(result, Err(Error::Cleanup(_))));
        assert_eq!(fs::read(root.join("unregistered")).unwrap(), b"retained");
        assert!(OwnedBundle::create(&root).is_err());

        let root = temp.path().join("child");
        let mut owned = OwnedBundle::create(&root).unwrap();
        let result = owned.directory_registered("originals", |path| {
            fs::write(path.join("unregistered"), b"child retained")?;
            Err(Error::Validation("injected registration failure".into()))
        });
        assert!(matches!(result, Err(Error::Cleanup(_))));
        assert!(owned.cleanup().is_err());
        assert_eq!(
            fs::read(root.join("originals/unregistered")).unwrap(),
            b"child retained"
        );
        assert!(OwnedBundle::create(&root).is_err());
    }
}
