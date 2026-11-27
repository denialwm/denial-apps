//! The public SDK boundary, the same for Denial's plugins and everyone
//! else's: no import of `denial_dart_shell` and none of another SDK
//! package's `src/`. Architecture validation from the directives' syntax
//! alone, not a sandbox for trusted Dart.

use std::path::{Path, PathBuf};

use crate::graph::Graph;
use crate::util::{Result, fail, is_within, normalize};

const SDK_PACKAGES: [&str; 2] = ["denial_sdk", "denial_flutter_sdk"];

/// The URIs of a Dart file's `import`, `export` and `part` directives,
/// configurations included.
pub fn directive_uris(source: &str) -> Vec<String> {
    let mut scanner = Scanner {
        text: source.as_bytes(),
        at: 0,
    };
    if source.starts_with("#!") {
        scanner.at = source.find('\n').unwrap_or(source.len());
    }
    let mut uris = Vec::new();
    loop {
        scanner.skip_blank();
        if scanner.peek() == Some(b'@') {
            scanner.at += 1;
            scanner.skip_annotation();
            continue;
        }
        let word = scanner.word();
        match word.as_str() {
            "import" | "export" => scanner.directive(&mut uris, true),
            "part" => {
                scanner.skip_blank();
                if scanner.lookahead_word() == "of" {
                    scanner.skip_statement();
                } else {
                    scanner.directive(&mut uris, false);
                }
            }
            "library" | "augment" => scanner.skip_statement(),
            _ => return uris,
        }
    }
}

struct Scanner<'a> {
    text: &'a [u8],
    at: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn skip_blank(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.at += 1;
            }
            if self.text[self.at..].starts_with(b"//") {
                while self.peek().is_some_and(|byte| byte != b'\n') {
                    self.at += 1;
                }
            } else if self.text[self.at..].starts_with(b"/*") {
                // Dart's block comments nest.
                let mut depth = 0;
                while self.at < self.text.len() {
                    if self.text[self.at..].starts_with(b"/*") {
                        depth += 1;
                        self.at += 2;
                    } else if self.text[self.at..].starts_with(b"*/") {
                        depth -= 1;
                        self.at += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        self.at += 1;
                    }
                }
            } else {
                return;
            }
        }
    }

    fn is_word(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(Self::is_word) {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.text[start..self.at]).into_owned()
    }

    fn lookahead_word(&self) -> String {
        let mut end = self.at;
        while self.text.get(end).copied().is_some_and(Self::is_word) {
            end += 1;
        }
        String::from_utf8_lossy(&self.text[self.at..end]).into_owned()
    }

    /// Skips `name(.name)*` and one balanced argument list.
    fn skip_annotation(&mut self) {
        loop {
            self.skip_blank();
            self.word();
            self.skip_blank();
            if self.peek() == Some(b'.') {
                self.at += 1;
            } else {
                break;
            }
        }
        if self.peek() == Some(b'<') {
            self.skip_balanced(b'<', b'>');
            self.skip_blank();
        }
        if self.peek() == Some(b'(') {
            self.skip_balanced(b'(', b')');
        }
    }

    fn skip_balanced(&mut self, open: u8, close: u8) {
        let mut depth = 0;
        while let Some(byte) = self.peek() {
            if byte == b'\''
                || byte == b'"'
                || (byte == b'r' && matches!(self.text.get(self.at + 1), Some(b'\'' | b'"')))
            {
                self.string();
                continue;
            }
            self.at += 1;
            if byte == open {
                depth += 1;
            } else if byte == close {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
        }
    }

    /// Reads a string literal at the cursor; None when there is none. Only
    /// what a directive can hold: no interpolation.
    fn string(&mut self) -> Option<String> {
        let raw = self.peek() == Some(b'r');
        let start = self.at + usize::from(raw);
        let quote = *self.text.get(start)?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        let triple = self.text[start..].starts_with(&[quote; 3]);
        let delimiter = if triple { 3 } else { 1 };
        let mut index = start + delimiter;
        let mut value = Vec::new();
        while index < self.text.len() {
            if self.text[index..].starts_with(&vec![quote; delimiter]) {
                self.at = index + delimiter;
                return Some(String::from_utf8_lossy(&value).into_owned());
            }
            let byte = self.text[index];
            if byte == b'\\' && !raw && index + 1 < self.text.len() {
                value.push(self.text[index + 1]);
                index += 2;
                continue;
            }
            if byte == b'\n' && !triple {
                break;
            }
            value.push(byte);
            index += 1;
        }
        self.at = self.text.len();
        None
    }

    /// Adjacent string literals, joined.
    fn strings(&mut self) -> Option<String> {
        let mut value = self.string()?;
        loop {
            self.skip_blank();
            let before = self.at;
            match self.string() {
                Some(next) => value.push_str(&next),
                None => {
                    self.at = before;
                    return Some(value);
                }
            }
        }
    }

    fn skip_statement(&mut self) {
        while let Some(byte) = self.peek() {
            if byte == b'\'' || byte == b'"' {
                self.string();
                continue;
            }
            self.at += 1;
            if byte == b';' {
                return;
            }
        }
    }

    /// Reads an `import`, `export` or `part` after its keyword.
    fn directive(&mut self, uris: &mut Vec<String>, configurable: bool) {
        self.skip_blank();
        if let Some(uri) = self.strings() {
            uris.push(uri);
        }
        loop {
            self.skip_blank();
            match self.peek() {
                None => return,
                Some(b';') => {
                    self.at += 1;
                    return;
                }
                Some(b'\'' | b'"') => {
                    self.string();
                }
                Some(b'(') => self.skip_balanced(b'(', b')'),
                Some(byte) if Self::is_word(byte) => {
                    let word = self.word();
                    if configurable && word == "if" {
                        self.skip_blank();
                        if self.peek() == Some(b'(') {
                            self.skip_balanced(b'(', b')');
                        }
                        self.skip_blank();
                        if let Some(uri) = self.strings() {
                            uris.push(uri);
                        }
                    }
                }
                Some(_) => self.at += 1,
            }
        }
    }
}

/// A URI's scheme, when it has one.
fn scheme(uri: &str) -> Option<&str> {
    let (scheme, _) = uri.split_once(':')?;
    let mut bytes = scheme.bytes();
    (bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'.' || b == b'-'))
    .then_some(scheme)
}

/// Fails when the file `path` of `package` reaches past the public SDK.
pub fn validate_imports(
    package: &str,
    path: &Path,
    source: &str,
    sdk_libraries: &[(String, PathBuf)],
) -> Result<()> {
    for value in directive_uris(source) {
        match scheme(&value) {
            Some("package") => {
                let rest = &value["package:".len()..];
                let segments: Vec<&str> = rest.split('/').collect();
                let owner = segments[0];
                if owner == "denial_dart_shell"
                    || (SDK_PACKAGES.contains(&owner)
                        && owner != package
                        && segments.len() > 1
                        && segments[1] == "src")
                {
                    return fail(format!(
                        "{} imports unsupported API {value}. Use a public SDK library.",
                        path.display()
                    ));
                }
            }
            None | Some("file") => {
                let reference = value.strip_prefix("file://").unwrap_or(&value);
                let target = if reference.starts_with('/') {
                    normalize(Path::new(reference))
                } else {
                    let folder = crate::util::absolute(path);
                    normalize(&folder.parent().unwrap_or(Path::new("/")).join(reference))
                };
                for (sdk, library) in sdk_libraries {
                    if sdk != package && is_within(&library.join("src"), &target) {
                        return fail(format!(
                            "{} reaches SDK internals through {value}. Use a public SDK library.",
                            path.display()
                        ));
                    }
                }
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn dart_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(listing) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dart_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "dart") && path.is_file() {
            files.push(path);
        }
    }
}

/// Checks every package that uses the SDK, directly or through others.
pub fn validate(graph: &Graph) -> Result<()> {
    if graph.package("denial_dart_shell").is_some() {
        return fail(
            "denial_dart_shell is an application, not a plugin API. Depend on denial_sdk and denial_flutter_sdk instead.",
        );
    }
    let mut clients: Vec<String> = SDK_PACKAGES.iter().map(|name| (*name).to_owned()).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for package in &graph.packages {
            if !clients.contains(&package.name)
                && package
                    .dependencies()
                    .iter()
                    .any(|dependency| clients.contains(dependency))
            {
                clients.push(package.name.clone());
                changed = true;
            }
        }
    }
    let sdk_libraries: Vec<(String, PathBuf)> = SDK_PACKAGES
        .iter()
        .filter_map(|name| {
            graph
                .package(name)
                .map(|package| ((*name).to_owned(), package.library_root.clone()))
        })
        .collect();
    for name in &clients {
        let Some(package) = graph.package(name) else {
            continue;
        };
        let mut files = Vec::new();
        dart_files(&package.library_root, &mut files);
        files.sort();
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap_or_default();
            validate_imports(name, &file, &source, &sdk_libraries)?;
        }
    }
    Ok(())
}
