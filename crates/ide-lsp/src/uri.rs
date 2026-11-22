//! `file://` URIs, as language servers name files, to and from paths.

use ide_core::RelPath;

/// Bytes kept as they are in a URI path; every other is `%XX`.
fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// The `file://` URI of an absolute path, as people write it (`C:\src\a b.ts`
/// or `/home/me/a.ts`).
pub fn to_uri(path: &str) -> String {
    let forward = path.replace('\\', "/");
    // `C:/x` → `/C:/x`: a URI path starts with a slash.
    let rooted = if forward.starts_with('/') {
        forward
    } else {
        format!("/{forward}")
    };
    let mut uri = String::from("file://");
    for (index, byte) in rooted.bytes().enumerate() {
        // The drive's colon stays (`/C:/`).
        if plain(byte) || (byte == b':' && index == 2) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The path of a `file://` URI, `%XX` decoded, with `/` separators (and no
/// leading slash before a Windows drive). `None` for any other kind of URI.
pub fn from_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // A host (`file://server/share`) isn't a local file.
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            // Two hex digits must follow; a cut-off `%` makes the URI invalid.
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    // `/c:/x` → `c:/x`.
    let windows_drive = path.len() >= 3
        && path.as_bytes()[0] == b'/'
        && path.as_bytes()[1].is_ascii_alphabetic()
        && path.as_bytes()[2] == b':';
    Some(if windows_drive {
        path[1..].to_owned()
    } else {
        path
    })
}

/// A path from a server as a path in the workspace at `root` (as people
/// write it), if it's inside. A Windows drive letter matches in any case.
pub fn relative_to(root: &str, path: &str) -> Option<RelPath> {
    let normalize = |text: &str| {
        let mut forward = text.replace('\\', "/");
        let bytes = forward.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' {
            let drive = forward[..1].to_ascii_lowercase();
            forward.replace_range(..1, &drive);
        }
        forward.trim_end_matches('/').to_owned()
    };
    let (root, path) = (normalize(root), normalize(path));
    let inside = path.strip_prefix(&root)?.strip_prefix('/')?;
    RelPath::parse(inside).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_unix_paths_become_uris() {
        assert_eq!(to_uri(r"C:\src\a b.ts"), "file:///C:/src/a%20b.ts");
        assert_eq!(to_uri("/home/me/#1/é.ts"), "file:///home/me/%231/%C3%A9.ts");
    }

    #[test]
    fn uris_become_paths_whatever_the_server_encoded() {
        assert_eq!(
            from_uri("file:///c%3A/Users/me/a%20b.ts").as_deref(),
            Some("c:/Users/me/a b.ts")
        );
        assert_eq!(from_uri("file:///C:/x.ts").as_deref(), Some("C:/x.ts"));
        assert_eq!(
            from_uri("file:///home/me/%C3%A9.ts").as_deref(),
            Some("/home/me/é.ts")
        );
        assert_eq!(from_uri("untitled:Untitled-1"), None);
        assert_eq!(from_uri("file://server/share/x.ts"), None);
    }

    #[test]
    fn round_trip() {
        for path in [r"C:\src\a b.ts", "/home/me/x y/é.ts"] {
            let back = from_uri(&to_uri(path)).unwrap();
            assert_eq!(back, path.replace('\\', "/"));
        }
    }

    #[test]
    fn only_paths_inside_the_workspace_are_relative() {
        let root = r"C:\Users\me\project";
        assert_eq!(
            relative_to(root, "c:/Users/me/project/src/a.ts").map(|p| p.as_str().to_owned()),
            Some("src/a.ts".to_owned())
        );
        assert_eq!(relative_to(root, "c:/Users/me/project-other/a.ts"), None);
        assert_eq!(relative_to(root, "c:/Users/me/project"), None);
        assert_eq!(
            relative_to("/home/me/p", "/home/me/p/lib/x.ts").map(|p| p.as_str().to_owned()),
            Some("lib/x.ts".to_owned())
        );
        assert_eq!(relative_to("/home/me/p", "/home/me/other.ts"), None);
    }
}
