use crate::json::{Json, JsonArray, JsonObject, JsonValue};

pub fn client_capabilities(json: &mut Json) -> JsonValue {
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

    capabilities.into()
}
