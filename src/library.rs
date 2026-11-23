//! Copying files and folders dropped from the OS into a library folder.

use std::path::{Path, PathBuf};

/// `name`, or `name (2).ext`, `name (3).ext` … when `dir` already has it.
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Copy one file or a whole folder into `dir`, never overwriting. Returns the new path.
pub fn copy_into(src: &Path, dir: &Path) -> Result<PathBuf, String> {
    let name = src.file_name().and_then(|n| n.to_str()).ok_or_else(|| format!("{}: no name", src.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // a folder dropped onto itself or into its own subtree would recurse forever
    if src.is_dir() && dir.canonicalize().ok().zip(src.canonicalize().ok()).is_some_and(|(d, s)| d.starts_with(s)) {
        return Err(format!("{name} cannot be copied into itself"));
    }
    let dst = free_name(dir, name);
    if src.is_dir() {
        copy_dir(src, &dst)?;
    } else {
        std::fs::copy(src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
    }
    Ok(dst)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for e in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?.flatten() {
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ventosa-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn collisions_get_a_counter() {
        let d = tmp("coll");
        std::fs::write(d.join("a.mp4"), b"1").unwrap();
        std::fs::write(d.join("a (2).mp4"), b"1").unwrap();
        assert_eq!(free_name(&d, "a.mp4"), d.join("a (3).mp4"));
        assert_eq!(free_name(&d, "b.mp4"), d.join("b.mp4"));
        std::fs::write(d.join("noext"), b"1").unwrap();
        assert_eq!(free_name(&d, "noext"), d.join("noext (2)"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_and_folders_are_copied_without_overwrite() {
        let d = tmp("copy");
        let (src, dst) = (d.join("src"), d.join("dst"));
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("x.txt"), b"new").unwrap();
        std::fs::write(src.join("sub/y.txt"), b"y").unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("x.txt"), b"old").unwrap();
        let p = copy_into(&src.join("x.txt"), &dst).unwrap();
        assert_eq!(p, dst.join("x (2).txt"));
        assert_eq!(std::fs::read(dst.join("x.txt")).unwrap(), b"old");
        let f = copy_into(&src, &dst).unwrap();
        assert_eq!(std::fs::read(f.join("sub/y.txt")).unwrap(), b"y");
        let f2 = copy_into(&src, &dst).unwrap();
        assert_eq!(f2, dst.join("src (2)"));
        assert!(copy_into(&src, &src.join("sub")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
