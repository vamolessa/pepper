use crate::json::{Json, JsonArray, JsonObject, JsonValue};

pub fn client_capabilities(json: &mut Json) -> JsonValue {
    fn symbol_kind(json: &mut Json) -> JsonObject {
        // https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_documentSymbol
        let mut symbol_kind = JsonObject::default();
        let mut value_set = JsonArray::default();
        for i in 1..=26 {
            value_set.push(JsonValue::Integer(i as _), json);
        }
        symbol_kind.set("valueSet".into(), value_set.into(), json);
        symbol_kind
    }

    fn tag_support(json: &mut Json) -> JsonObject {
        // https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_completion
        let mut tag_support = JsonObject::default();
        let mut value_set = JsonArray::default();
        value_set.push(JsonValue::Integer(1), json);
        tag_support.set("valueSet".into(), value_set.into(), json);
        tag_support
    }

    fn completion_item_kind(json: &mut Json) -> JsonObject {
        // https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_completion
        let mut completion_item_kind = JsonObject::default();
        let mut value_set = JsonArray::default();
        for i in 1..=25 {
            value_set.push(JsonValue::Integer(i as _), json);
        }
        completion_item_kind.set("valueSet".into(), value_set.into(), json);
        completion_item_kind
    }

    fn code_action_kind(json: &mut Json) -> JsonObject {
        // https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_codeAction
        let mut code_action_kind = JsonObject::default();
        let mut value_set = JsonArray::default();
        value_set.push("".into(), json);
        value_set.push("quickfix".into(), json);
        value_set.push("refactor".into(), json);
        value_set.push("refactor.extract".into(), json);
        value_set.push("refactor.inline".into(), json);
        value_set.push("refactor.rewrite".into(), json);
        value_set.push("source".into(), json);
        value_set.push("source.organizeImports".into(), json);
        code_action_kind.set("valueSet".into(), value_set.into(), json);
        code_action_kind
    }

    let mut capabilities = JsonObject::default();

    {
        let mut workspace_capabilities = JsonObject::default();
        workspace_capabilities.set("applyEdit".into(), true.into(), json);
        workspace_capabilities.set("workspaceFolders".into(), false.into(), json);
        workspace_capabilities.set("configuration".into(), false.into(), json);

        workspace_capabilities.set(
            "didChangeWatchedFiles".into(),
            JsonObject::default().into(),
            json,
        );

        workspace_capabilities.set("executeCommand".into(), JsonObject::default().into(), json);

        {
            let mut workspace_edit_capabilities = JsonObject::default();
            workspace_edit_capabilities.set("documentChanges".into(), true.into(), json);
            workspace_edit_capabilities.set("failureHandling".into(), "undo".into(), json);

            let mut resource_operation_kinds = JsonArray::default();
            resource_operation_kinds.push("create".into(), json);
            resource_operation_kinds.push("rename".into(), json);
            resource_operation_kinds.push("delete".into(), json);
            workspace_edit_capabilities.set(
                "resourceOperations".into(),
                resource_operation_kinds.into(),
                json,
            );

            let mut change_annotation_support = JsonObject::default();
            change_annotation_support.set("groupsOnLabel".into(), false.into(), json);
            workspace_edit_capabilities.set(
                "changeAnnotationSupport".into(),
                change_annotation_support.into(),
                json,
            );

            workspace_capabilities.set(
                "workspaceEdit".into(),
                workspace_edit_capabilities.into(),
                json,
            );
        }

        {
            let mut symbol = JsonObject::default();
            symbol.set("symbolKind".into(), symbol_kind(json).into(), json);

            workspace_capabilities.set("symbol".into(), symbol.into(), json);
        }

        capabilities.set("workspace".into(), workspace_capabilities.into(), json);
    }

    {
        let mut text_document_capabilities = JsonObject::default();

        {
            let mut synchronization = JsonObject::default();
            synchronization.set("willSave".into(), false.into(), json);
            synchronization.set("willSaveWaitUntil".into(), false.into(), json);
            synchronization.set("didSave".into(), true.into(), json);

            text_document_capabilities.set("synchronization".into(), synchronization.into(), json);
        }

        {
            let mut completion = JsonObject::default();
            completion.set("contextSupport".into(), false.into(), json);

            {
                let mut completion_item = JsonObject::default();
                completion_item.set("snippetSupport".into(), false.into(), json);
                completion_item.set("commitCharactersSupport".into(), false.into(), json);

                let mut documentation_formats = JsonArray::default();
                documentation_formats.push("plaintext".into(), json);
                completion_item.set(
                    "documentationFormat".into(),
                    documentation_formats.into(),
                    json,
                );

                completion_item.set("deprecatedSupport".into(), false.into(), json);
                completion_item.set("preselectSupport".into(), false.into(), json);
                completion_item.set("tagSupport".into(), tag_support(json).into(), json);

                completion.set("completionItem".into(), completion_item.into(), json);
            }

            completion.set(
                "completionItemKind".into(),
                completion_item_kind(json).into(),
                json,
            );

            text_document_capabilities.set("completion".into(), completion.into(), json);
        }

        {
            let mut hover = JsonObject::default();
            let mut content_formats = JsonArray::default();
            content_formats.push("plaintext".into(), json);
            hover.set("contentFormat".into(), content_formats.into(), json);

            text_document_capabilities.set("hover".into(), hover.into(), json);
        }

        {
            let mut signature_help = JsonObject::default();
            signature_help.set("contextSupport".into(), false.into(), json);

            {
                let mut signature_information = JsonObject::default();

                let mut documentation_formats = JsonArray::default();
                documentation_formats.push("plaintext".into(), json);
                signature_information.set(
                    "documentationFormat".into(),
                    documentation_formats.into(),
                    json,
                );

                let mut parameter_information = JsonObject::default();
                parameter_information.set("labelOffsetSupport".into(), false.into(), json);
                signature_information.set(
                    "parameterInformation".into(),
                    parameter_information.into(),
                    json,
                );

                signature_help.set(
                    "signatureInformation".into(),
                    signature_information.into(),
                    json,
                );
            }

            text_document_capabilities.set("signatureHelp".into(), signature_help.into(), json);
        }

        {
            let mut declaration = JsonObject::default();
            declaration.set("linkSupport".into(), false.into(), json);

            text_document_capabilities.set("declaration".into(), declaration.into(), json);
        }

        {
            let mut definition = JsonObject::default();
            definition.set("linkSupport".into(), false.into(), json);

            text_document_capabilities.set("definition".into(), definition.into(), json);
        }

        {
            let mut type_definition = JsonObject::default();
            type_definition.set("linkSupport".into(), false.into(), json);

            text_document_capabilities.set("typeDefinition".into(), type_definition.into(), json);
        }

        {
            let mut implementation = JsonObject::default();
            implementation.set("linkSupport".into(), false.into(), json);

            text_document_capabilities.set("implementation".into(), implementation.into(), json);
        }

        text_document_capabilities.set("references".into(), JsonObject::default().into(), json);

        {
            let mut document_symbol = JsonObject::default();
            document_symbol.set("symbolKind".into(), symbol_kind(json).into(), json);

            text_document_capabilities.set("documentSymbol".into(), document_symbol.into(), json);
        }

        {
            let mut code_action = JsonObject::default();

            {
                let mut code_action_literal_support = JsonObject::default();
                code_action_literal_support.set(
                    "codeActionKind".into(),
                    code_action_kind(json).into(),
                    json,
                );

                code_action.set(
                    "codeActionLiteralSupport".into(),
                    code_action_literal_support.into(),
                    json,
                );
            }

            text_document_capabilities.set("codeAction".into(), code_action.into(), json);
        }

        {
            let mut document_link = JsonObject::default();
            document_link.set("tooltipSupport".into(), false.into(), json);

            text_document_capabilities.set("documentLink".into(), document_link.into(), json);
        }

        text_document_capabilities.set("formatting".into(), JsonObject::default().into(), json);
        text_document_capabilities.set(
            "rangeFormatting".into(),
            JsonObject::default().into(),
            json,
        );

        {
            let mut rename = JsonObject::default();
            rename.set("prepareSupport".into(), true.into(), json);
            rename.set("prepareSupportDefaultBehavior".into(), 1.into(), json);
            rename.set("honorsChangeAnnotations".into(), false.into(), json);

            text_document_capabilities.set("rename".into(), rename.into(), json);
        }

        {
            let mut publish_diagnostics = JsonObject::default();
            publish_diagnostics.set("relatedInformation".into(), true.into(), json);
            publish_diagnostics.set("tagSupport".into(), tag_support(json).into(), json);
            publish_diagnostics.set("versionSupport".into(), false.into(), json);

            text_document_capabilities.set(
                "publishDiagnostics".into(),
                publish_diagnostics.into(),
                json,
            );
        }

        text_document_capabilities.set("selectionRange".into(), JsonObject::default().into(), json);

        capabilities.set(
            "textDocument".into(),
            text_document_capabilities.into(),
            json,
        );
    }

    capabilities.into()
}
