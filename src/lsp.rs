mod capabilities;
mod client;
mod protocol;

pub type LspClientHandle = client::ClientHandle;
pub type LspClientCollection = client::ClientCollection;
pub type LspClientContext<'a> = client::ClientContext<'a>;
pub type LspServerEvent = protocol::ServerEvent;
