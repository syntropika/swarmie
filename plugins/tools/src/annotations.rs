/// Author-supplied descriptive hints. Neither these values nor footprints grant
/// execution permission, automatic retry, concurrency or sandbox guarantees.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolAnnotations {
    title: Option<String>,
    read_only: Option<bool>,
    destructive: Option<bool>,
    idempotent: Option<bool>,
    open_world: Option<bool>,
}

impl ToolAnnotations {
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn with_read_only(mut self, value: bool) -> Self {
        self.read_only = Some(value);
        self
    }
    pub fn with_destructive(mut self, value: bool) -> Self {
        self.destructive = Some(value);
        self
    }
    pub fn with_idempotent(mut self, value: bool) -> Self {
        self.idempotent = Some(value);
        self
    }
    pub fn with_open_world(mut self, value: bool) -> Self {
        self.open_world = Some(value);
        self
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn read_only(&self) -> bool {
        self.read_only.unwrap_or(false)
    }
    pub fn destructive(&self) -> bool {
        self.destructive.unwrap_or(true)
    }
    pub fn idempotent(&self) -> bool {
        self.idempotent.unwrap_or(false)
    }
    pub fn open_world(&self) -> bool {
        self.open_world.unwrap_or(true)
    }
}
