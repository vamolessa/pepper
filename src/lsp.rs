mod capabilities;
mod client;
mod protocol;

pub type LspClientHandle = client::ClientHandle;
pub type LspClientCollection = client::ClientCollection;
pub type LspServerEvent = protocol::ServerEvent;
