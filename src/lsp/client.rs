use std::{env, io, process};

use crate::json::{JsonArray, JsonObject, JsonValue};

use super::protocol::{Protocol, ServerConnection};

pub struct Client {
    protocol: Protocol,
}

impl Client {
    pub fn new(server_executable: &str) -> io::Result<Self> {
        let server_command = process::Command::new(server_executable);
        let connection = ServerConnection::spawn(server_command)?;
        Ok(Self {
            protocol: Protocol::new(connection),
        })
    }

    pub fn initialize(&mut self) -> io::Result<()> {
        let json = &mut self.protocol.json;

        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => JsonValue::String(json.create_string(path)),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::new();
        params.push(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );
        params.push("rootUri".into(), current_dir, json);

        let mut workspace_capabilities = JsonObject::new();
        {
            workspace_capabilities.push("applyEdit".into(), JsonValue::Boolean(true), json);

            let mut workspace_edit_capabilities = JsonObject::new();
            {
                workspace_edit_capabilities.push(
                    "documentChanges".into(),
                    JsonValue::Boolean(true),
                    json,
                );

                let mut resource_operation_kinds = JsonArray::new();
                resource_operation_kinds.push("create".into(), json);
                resource_operation_kinds.push("rename".into(), json);
                resource_operation_kinds.push("delete".into(), json);
                workspace_edit_capabilities.push(
                    "resourceOperations".into(),
                    resource_operation_kinds.into(),
                    json,
                );

                let mut failure_handling_kinds = JsonArray::new();
                failure_handling_kinds.push("abort".into(), json);
                failure_handling_kinds.push("undo".into(), json);
                workspace_edit_capabilities.push(
                    "failureHandling".into(),
                    failure_handling_kinds.into(),
                    json,
                );
            }
        }

        let mut text_document_capabilities = JsonObject::new();

        let mut capabilities = JsonObject::new();
        capabilities.push("workspace".into(), workspace_capabilities.into(), json);
        capabilities.push(
            "textDocument".into(),
            text_document_capabilities.into(),
            json,
        );

        params.push("capabilities".into(), JsonValue::Object(capabilities), json);

        self.protocol.request("initialize", params.into())
    }

    pub fn wait_response(&mut self) -> io::Result<&str> {
        self.protocol.wait_response()
    }
}
