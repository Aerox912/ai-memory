//! Bounded process-only secret inputs.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::Path;

use anyhow::{Context, Result, bail};

const MAX_SECRET_BYTES: u64 = 64 * 1024;

/// Read one trimmed, non-empty UTF-8 secret from a file or pipe path.
pub fn read_file(path: &Path, label: &str) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("opening {label} secret file {}", path.display()))?;
    read_bounded(file, label)
}

/// Resolve exactly one direct value or file-backed value.
pub fn resolve(direct: Option<String>, file: Option<&Path>, label: &str) -> Result<Option<String>> {
    if direct.is_some() && file.is_some() {
        bail!("{label} was supplied both directly and through a secret file");
    }
    file.map(|path| read_file(path, label))
        .transpose()
        .map(|value| value.or(direct))
}

/// Read a secret from stdin. Used only by the hidden detached hook drainer.
pub fn read_stdin(label: &str) -> Result<String> {
    read_bounded(std::io::stdin().lock(), label)
}

/// Create an anonymous, rewound file suitable for a child's stdin.
pub fn anonymous_file(secret: &str, label: &str) -> Result<File> {
    let mut file = tempfile::tempfile().with_context(|| format!("creating {label} secret pipe"))?;
    file.write_all(secret.as_bytes())
        .with_context(|| format!("writing {label} secret pipe"))?;
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("rewinding {label} secret pipe"))?;
    Ok(file)
}

fn read_bounded(reader: impl std::io::Read, label: &str) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_SECRET_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {label} secret"))?;
    if bytes.len() as u64 > MAX_SECRET_BYTES {
        bail!("{label} secret exceeds {MAX_SECRET_BYTES} bytes");
    }
    let value = String::from_utf8(bytes).with_context(|| format!("{label} secret is not UTF-8"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{label} secret is empty");
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_secret_is_trimmed_and_direct_file_ambiguity_fails() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, " secret-value ").unwrap();
        assert_eq!(read_file(file.path(), "test").unwrap(), "secret-value");
        assert!(resolve(Some("direct".into()), Some(file.path()), "test").is_err());
    }

    #[test]
    fn empty_and_oversized_secrets_fail_closed() {
        assert!(read_bounded(" \n".as_bytes(), "test").is_err());
        let oversized = vec![b'x'; (MAX_SECRET_BYTES + 1) as usize];
        assert!(read_bounded(oversized.as_slice(), "test").is_err());
    }

    #[test]
    fn anonymous_secret_file_is_rewound() {
        let file = anonymous_file("secret-value", "test").unwrap();
        assert_eq!(read_bounded(file, "test").unwrap(), "secret-value");
    }
}
