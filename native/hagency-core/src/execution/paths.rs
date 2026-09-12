use super::bounded;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathFlavor {
    Posix,
    Windows,
}
impl PathFlavor {
    /// Lexical matching context only, without filesystem IO or containment proof.
    pub fn normalize(self, value: &str) -> Option<String> {
        if !bounded(value) {
            return None;
        }
        match self {
            Self::Posix => {
                if !value.starts_with('/') {
                    return None;
                }
                Some(normalize_tail("/", value, '/', value.ends_with('/')))
            }
            Self::Windows => {
                let value = value.replace('/', "\\");
                let bytes = value.as_bytes();
                let (root, tail) = if bytes.len() >= 3
                    && bytes[0].is_ascii_alphabetic()
                    && bytes[1] == b':'
                    && bytes[2] == b'\\'
                {
                    (value[..3].to_owned(), &value[3..])
                } else if let Some(unc) = value.strip_prefix("\\\\") {
                    let (server, rest) = unc.split_once('\\')?;
                    if server.is_empty() || [".", "?", ".."].contains(&server) {
                        return None;
                    }
                    let rest = rest.trim_start_matches('\\');
                    let (share, tail) = rest.split_once('\\').unwrap_or((rest, ""));
                    if share.is_empty() || [".", ".."].contains(&share) {
                        return None;
                    }
                    (format!("\\\\{server}\\{share}\\"), tail)
                } else {
                    return None;
                };
                Some(normalize_tail(&root, tail, '\\', value.ends_with('\\')))
            }
        }
    }
}
fn normalize_tail(root: &str, value: &str, separator: char, trailing: bool) -> String {
    let mut components = Vec::new();
    for part in value.split(separator) {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            _ => components.push(part),
        }
    }
    let mut result = root.to_owned();
    result.push_str(&components.join(&separator.to_string()));
    if trailing && !components.is_empty() {
        result.push(separator);
    }
    result
}
