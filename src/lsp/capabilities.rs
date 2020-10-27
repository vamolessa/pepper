use crate::json::{Json, JsonArray, JsonObject, JsonValue};

pub fn client_capabilities(json: &mut Json) -> JsonValue {
    let mut capabilities = JsonObject::new();

    {
        let mut workspace_capabilities = JsonObject::new();
        workspace_capabilities.set("applyEdit".into(), true.into(), json);
        workspace_capabilities.set("workspaceFolders".into(), false.into(), json);
        workspace_capabilities.set("configuration".into(), false.into(), json);

        workspace_capabilities.set(
            "didChangeWatchedFiles".into(),
            JsonObject::new().into(),
            json,
        );

        workspace_capabilities.set("executeCommand".into(), JsonObject::new().into(), json);

        {
            let mut workspace_edit_capabilities = JsonObject::new();
            workspace_edit_capabilities.set("documentChanges".into(), true.into(), json);
            workspace_edit_capabilities.set("failureHandling".into(), "undo".into(), json);

            let mut resource_operation_kinds = JsonArray::new();
            resource_operation_kinds.push("create".into(), json);
            resource_operation_kinds.push("rename".into(), json);
            resource_operation_kinds.push("delete".into(), json);
            workspace_edit_capabilities.set(
                "resourceOperations".into(),
                resource_operation_kinds.into(),
                json,
            );

            workspace_capabilities.set(
                "workspaceEdit".into(),
                workspace_edit_capabilities.into(),
                json,
            );
        }

        {
            let mut symbol = JsonObject::new();
            symbol.set("symbolKind".into(), JsonObject::new().into(), json);

            workspace_capabilities.set("symbol".into(), symbol.into(), json);
        }

        capabilities.set("workspace".into(), workspace_capabilities.into(), json);
    }

    {
        let mut text_document_capabilities = JsonObject::new();

        {
            let mut synchronization = JsonObject::new();
            synchronization.set("willSave".into(), false.into(), json);
            synchronization.set("willSaveWaitUntil".into(), false.into(), json);
            synchronization.set("didSave".into(), false.into(), json);

            text_document_capabilities.set("synchronization".into(), synchronization.into(), json);
        }

        {
            let mut completion = JsonObject::new();

            {
                let mut completion_item = JsonObject::new();
                completion_item.set("snippetSupport".into(), false.into(), json);
                completion_item.set("commitCharactersSupport".into(), false.into(), json);

                completion.set("completionItem".into(), completion_item.into(), json);
            }

            text_document_capabilities.set("completion".into(), completion.into(), json);
        }

        capabilities.set(
            "textDocument".into(),
            text_document_capabilities.into(),
            json,
        );
    }

    capabilities.into()
}
