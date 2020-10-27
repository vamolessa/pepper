use crate::json::{Json, JsonArray, JsonObject, JsonValue};

pub fn client_capabilities(json: &mut Json) -> JsonValue {
    let mut capabilities = JsonObject::new();

    {
        let mut workspace_capabilities = JsonObject::new();
        workspace_capabilities.push("applyEdit".into(), true.into(), json);
        workspace_capabilities.push("workspaceFolders".into(), false.into(), json);
        workspace_capabilities.push("configuration".into(), false.into(), json);

        {
            let mut workspace_edit_capabilities = JsonObject::new();
            workspace_edit_capabilities.push("documentChanges".into(), true.into(), json);
            workspace_edit_capabilities.push("failureHandling".into(), "undo".into(), json);

            let mut resource_operation_kinds = JsonArray::new();
            resource_operation_kinds.push("create".into(), json);
            resource_operation_kinds.push("rename".into(), json);
            resource_operation_kinds.push("delete".into(), json);
            workspace_edit_capabilities.push(
                "resourceOperations".into(),
                resource_operation_kinds.into(),
                json,
            );

            workspace_capabilities.push(
                "workspaceEdit".into(),
                workspace_edit_capabilities.into(),
                json,
            );
        }

        {
            let mut did_change_watched_files = JsonObject::new();
            did_change_watched_files.push("dynamicRegistration".into(), false.into(), json);

            workspace_capabilities.push(
                "didChangeWatchedFiles".into(),
                did_change_watched_files.into(),
                json,
            );
        }

        {
            let mut symbol = JsonObject::new();
            symbol.push("dynamicRegistration".into(), false.into(), json);
            symbol.push("symbolKind".into(), JsonObject::new().into(), json);

            workspace_capabilities.push("symbol".into(), symbol.into(), json);
        }

        {
            let mut execute_command = JsonObject::new();
            execute_command.push("dynamicRegistration".into(), false.into(), json);
            workspace_capabilities.push("executeCommand".into(), execute_command.into(), json);
        }

        capabilities.push("workspace".into(), workspace_capabilities.into(), json);
    }

    {
        let mut text_document_capabilities = JsonObject::new();
        capabilities.push(
            "textDocument".into(),
            text_document_capabilities.into(),
            json,
        );
    }

    capabilities.into()
}
