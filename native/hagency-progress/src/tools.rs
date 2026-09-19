/// Typed host observations, not a runtime protocol parser or execution proof.
/// Unknown and pending remain unresolved until explicit terminal evidence.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Pending,
    Completed,
    Failed,
    Unknown,
}
pub struct ToolEvent<'a> {
    pub(crate) id: &'a str,
    pub(crate) tool: Option<&'a str>,
    pub(crate) start: bool,
    pub(crate) state: ToolState,
}
impl<'a> ToolEvent<'a> {
    pub fn started(id: &'a str, tool: &'a str, state: ToolState) -> Self {
        Self {
            id,
            tool: Some(tool),
            start: true,
            state,
        }
    }
    pub fn updated(id: &'a str, state: ToolState) -> Self {
        Self {
            id,
            tool: None,
            start: false,
            state,
        }
    }
}
