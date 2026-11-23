//! 对调度导出和模型使用同一份源码完整性证明。

use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Component, Path},
};

pub fn fingerprint(root: &Path, source_inputs: Option<&[String]>) -> io::Result<String> {
    let mut hash = Sha256::new();
    if let Some(inputs) = source_inputs {
        let root = root.canonicalize()?;
        let mut paths: Vec<_> = inputs.iter().map(String::as_str).collect();
        paths.sort_unstable();
        if paths.windows(2).any(|pair| pair[0] == pair[1])
            || !paths.contains(&"Cargo.toml")
            || !paths.contains(&"Cargo.lock")
        {
            return Err(invalid("Invalid workspace source input list"));
        }
        for file in paths {
            let relative = Path::new(file);
            if file
                .split('/')
                .any(|component| component.is_empty() || matches!(component, "." | ".."))
                || file
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '\\' | ':'))
                || relative
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(invalid("Unsafe workspace source input path"));
            }
            let filename = root.join(relative);
            let canonical = filename.canonicalize()?;
            if !canonical.starts_with(&root)
                || fs::symlink_metadata(&filename)?.file_type().is_symlink()
                || !canonical.is_file()
            {
                return Err(invalid(
                    "Workspace source input escapes the project or is not a file",
                ));
            }
            let content = fs::read(canonical)?;
            hash.update(file.as_bytes());
            hash.update([0]);
            hash.update(content.len().to_string().as_bytes());
            hash.update([0]);
            hash.update(content);
        }
    } else {
        let mut paths = vec!["Cargo.toml".to_owned(), "Cargo.lock".to_owned()];
        if root.join("build.rs").is_file() {
            paths.push("build.rs".into());
        }
        collect_rust_files(root, Path::new("src"), &mut paths)?;
        paths.sort_unstable();
        for file in paths {
            hash.update(file.as_bytes());
            hash.update([0]);
            hash.update(fs::read_to_string(root.join(&file))?.replace("\r\n", "\n"));
            hash.update([0]);
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn collect_rust_files(root: &Path, relative: &Path, paths: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(invalid(&format!(
                "Source fingerprint does not support symlinks: {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_rust_files(root, &path, paths)?;
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs")
        {
            paths.push(
                path.to_str()
                    .ok_or_else(|| invalid("Source path is not valid UTF-8"))?
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
