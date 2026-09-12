use crate::Error;

/// Selection data only. Constructing this value grants no filesystem authority.
/// Portable UTF-8 names deliberately reject platform aliases, not normalize them.
pub struct RelativeFile {
    pub(crate) components: Vec<String>,
}
impl RelativeFile {
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 4096
            || value.chars().any(|c| {
                c.is_control() || matches!(c, '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*')
            })
        {
            return Err(Error::Selection);
        }
        let mut components = Vec::new();
        for component in value.split('/') {
            if component.is_empty()
                || component.len() > 255
                || component == "."
                || component == ".."
                || component.ends_with(['.', ' '])
                || reserved(component)
            {
                return Err(Error::Selection);
            }
            components.push(component.to_owned());
            if components.len() > 32 {
                return Err(Error::Selection);
            }
        }
        Ok(Self { components })
    }
}
fn reserved(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|rest| {
            matches!(
                rest,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}
