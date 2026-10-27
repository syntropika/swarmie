pub(crate) fn invalid(message: impl ToString) -> tembra::Error {
    tembra::Error::invalid(Reason::Invalid.as_str(), message.to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    Invalid,
    Duplicate,
    Missing,
}

impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "swarmie.tools.invalid",
            Self::Duplicate => "swarmie.tools.duplicate",
            Self::Missing => "swarmie.tools.missing",
        }
    }
}
