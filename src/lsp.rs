mod capabilities;
mod client;
mod protocol;

pub type LspClient = client::Client;
pub type LspServerMessage = protocol::ServerMessage;
