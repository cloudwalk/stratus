use std::fmt::Display;

#[derive(Debug, Clone, Default)]
pub enum RpcClientApp {
    /// Client application identified itself.
    Identified(String),

    /// Client application is unknown.
    #[default]
    Unknown,
}

impl Display for RpcClientApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcClientApp::Identified(name) => write!(f, "{}", name),
            RpcClientApp::Unknown => write!(f, "unknown"),
        }
    }
}
