use std::{cmp::Ord, fmt, fs::File, io, path::Path};

use pepper::{
    buffer::{BufferContent, BufferProperties},
    buffer_position::{BufferPosition, BufferRange},
    client,
    cursor::Cursor,
    editor::EditorContext,
    editor_utils::MessageKind,
    glob::Glob,
    mode::ModeKind,
    picker::Picker,
    plugin::PluginHandle,
    word_database::WordKind,
};

use crate::{
    client::{util, Client, RequestState, ServerCapabilities},
    json::{
        FromJson, Json, JsonArray, JsonConvertError, JsonInteger, JsonObject, JsonString, JsonValue,
    },
    mode::{picker, read_line},
    protocol::{
        DocumentCodeAction, DocumentCompletionItem, DocumentDiagnostic, DocumentLocation,
        DocumentPosition, DocumentRange, DocumentSymbolInformation, ProtocolError,
        ServerNotification, ServerRequest, ServerResponse, TextEdit, Uri, WorkspaceEdit,
    },
};

pub(crate) fn on_request(
    client: &mut Client,
    ctx: &mut EditorContext,
    request: ServerRequest,
) -> Result<JsonValue, ProtocolError> {
    client.write_to_log_file(|buf, json| {
        use io::Write;
        let _ = write!(buf, "receive request\nid: ");
        let _ = json.write(buf, &request.id);
        let _ = write!(
            buf,
            "\nmethod: '{}'\nparams:\n",
            request.method.as_str(json)
        );
        let _ = json.write(buf, &request.params);
    });

    match request.method.as_str(&client.json) {
        "client/registerCapability" => {
            for registration in request
                .params
                .get("registrations", &client.json)
                .elements(&client.json)
            {
                #[derive(Default)]
                struct Registration {
                    method: JsonString,
                    register_options: JsonObject,
                }
                impl<'json> FromJson<'json> for Registration {
                    fn from_json(
                        value: JsonValue,
                        json: &'json Json,
                    ) -> Result<Self, JsonConvertError> {
                        let mut this = Self::default();
                        for (key, value) in value.members(json) {
                            match key {
                                "method" => this.method = JsonString::from_json(value, json)?,
                                "registerOptions" => {
                                    this.register_options = JsonObject::from_json(value, json)?
                                }
                                _ => (),
                            }
                        }
                        Ok(this)
                    }
                }

                struct Filter {
                    pattern: Option<JsonString>,
                }
                impl<'json> FromJson<'json> for Filter {
                    fn from_json(
                        value: JsonValue,
                        json: &'json Json,
                    ) -> Result<Self, JsonConvertError> {
                        let pattern = value.get("pattern", json);
                        Ok(Self {
                            pattern: FromJson::from_json(pattern, json)?,
                        })
                    }
                }

                let registration = Registration::from_json(registration, &client.json)?;
                match registration.method.as_str(&client.json) {
                    "textDocument/didSave" => {
                        client.document_selectors.clear();
                        for filter in registration
                            .register_options
                            .get("documentSelector", &client.json)
                            .elements(&client.json)
                        {
                            let filter = Filter::from_json(filter, &client.json)?;
                            let pattern = match filter.pattern {
                                Some(pattern) => pattern.as_str(&client.json),
                                None => continue,
                            };
                            let mut glob = Glob::default();
                            glob.compile(pattern)?;
                            client.document_selectors.push(glob);
                        }
                    }
                    _ => (),
                }
            }
            Ok(JsonValue::Null)
        }
        "window/showMessage" => {
            fn parse_params(
                params: JsonValue,
                json: &Json,
            ) -> Result<(MessageKind, &str), JsonConvertError> {
                let params = match params {
                    JsonValue::Object(object) => object,
                    _ => return Err(JsonConvertError),
                };
                let mut kind = MessageKind::Info;
                let mut message = "";
                for (key, value) in params.members(json) {
                    match key {
                        "type" => {
                            kind = match value {
                                JsonValue::Integer(1) => MessageKind::Error,
                                JsonValue::Integer(2..=4) => MessageKind::Info,
                                _ => return Err(JsonConvertError),
                            }
                        }
                        "message" => {
                            message = match value {
                                JsonValue::String(string) => string.as_str(json),
                                _ => return Err(JsonConvertError),
                            }
                        }
                        _ => (),
                    }
                }

                Ok((kind, message))
            }

            let (kind, message) = parse_params(request.params, &client.json)?;
            ctx.editor.status_bar.write(kind).str(message);
            Ok(JsonValue::Null)
        }
        "window/showDocument" => {
            #[derive(Default)]
            struct ShowDocumentParams {
                uri: JsonString,
                external: Option<bool>,
                take_focus: Option<bool>,
                selection: Option<DocumentRange>,
            }
            impl<'json> FromJson<'json> for ShowDocumentParams {
                fn from_json(
                    value: JsonValue,
                    json: &'json Json,
                ) -> Result<Self, JsonConvertError> {
                    let mut this = Self::default();
                    for (key, value) in value.members(json) {
                        match key {
                            "key" => this.uri = JsonString::from_json(value, json)?,
                            "external" => this.external = FromJson::from_json(value, json)?,
                            "takeFocus" => this.take_focus = FromJson::from_json(value, json)?,
                            "selection" => this.selection = FromJson::from_json(value, json)?,
                            _ => (),
                        }
                    }
                    Ok(this)
                }
            }

            let params = ShowDocumentParams::from_json(request.params, &client.json)?;
            let Uri::Path(path) = Uri::parse(&client.root, params.uri.as_str(&client.json))?;

            let success = if let Some(true) = params.external {
                false
            } else if let Some(client_handle) = ctx.clients.focused_client() {
                match ctx.editor.buffer_view_handle_from_path(
                    client_handle,
                    path,
                    BufferProperties::text(),
                    false,
                ) {
                    Ok(buffer_view_handle) => {
                        if let Some(true) = params.take_focus {
                            let client = ctx.clients.get_mut(client_handle);
                            client.set_buffer_view_handle(
                                Some(buffer_view_handle),
                                &ctx.editor.buffer_views,
                            );
                        }
                        if let Some(range) = params.selection {
                            let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
                            let mut cursors = buffer_view.cursors.mut_guard();
                            cursors.clear();
                            cursors.add(Cursor {
                                anchor: range.start.into_buffer_position(),
                                position: range.end.into_buffer_position(),
                            });
                        }
                        true
                    }
                    Err(error) => {
                        ctx.editor
                            .status_bar
                            .write(MessageKind::Error)
                            .fmt(format_args!("{}", error));
                        false
                    }
                }
            } else {
                false
            };

            let mut result = JsonObject::default();
            result.set("success".into(), success.into(), &mut client.json);
            Ok(result.into())
        }
        _ => Err(ProtocolError::MethodNotFound),
    }
}

pub(crate) fn on_notification(
    client: &mut Client,
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    notification: ServerNotification,
) -> Result<(), ProtocolError> {
    client.write_to_log_file(|buf, json| {
        use io::Write;
        let _ = write!(
            buf,
            "receive notification\nmethod: '{}'\nparams:\n",
            notification.method.as_str(json)
        );
        let _ = json.write(buf, &notification.params);
    });

    match notification.method.as_str(&client.json) {
        "window/showMessage" => {
            let mut message_type: JsonInteger = 0;
            let mut message = JsonString::default();
            for (key, value) in notification.params.members(&client.json) {
                match key {
                    "type" => message_type = JsonInteger::from_json(value, &client.json)?,
                    "value" => message = JsonString::from_json(value, &client.json)?,
                    _ => (),
                }
            }
            let message = message.as_str(&client.json);
            match message_type {
                1 => ctx.editor.status_bar.write(MessageKind::Error).str(message),
                2 => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Info)
                    .fmt(format_args!("warning: {}", message)),
                3 => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Info)
                    .fmt(format_args!("info: {}", message)),
                4 => ctx.editor.status_bar.write(MessageKind::Info).str(message),
                _ => (),
            }
            Ok(())
        }
        "textDocument/publishDiagnostics" => {
            #[derive(Default)]
            struct Params {
                uri: JsonString,
                diagnostics: JsonArray,
            }
            impl<'json> FromJson<'json> for Params {
                fn from_json(
                    value: JsonValue,
                    json: &'json Json,
                ) -> Result<Self, JsonConvertError> {
                    let mut this = Self::default();
                    for (key, value) in value.members(json) {
                        match key {
                            "uri" => this.uri = JsonString::from_json(value, json)?,
                            "diagnostics" => this.diagnostics = JsonArray::from_json(value, json)?,
                            _ => (),
                        }
                    }
                    Ok(this)
                }
            }

            let params = Params::from_json(notification.params, &client.json)?;
            let uri = params.uri.as_str(&client.json);
            let Uri::Path(path) = Uri::parse(&client.root, uri)?;

            let mut buffer_handle = None;
            for buffer in ctx.editor.buffers.iter() {
                if util::is_editor_path_equals_to_lsp_path(
                    &ctx.editor.current_directory,
                    &buffer.path,
                    &client.root,
                    path,
                ) {
                    buffer_handle = Some(buffer.handle());
                    break;
                }
            }
            if let Some(buffer_handle) = buffer_handle {
                let mut lints = ctx
                    .editor
                    .buffers
                    .get_mut(buffer_handle)
                    .lints
                    .mut_guard(plugin_handle);
                lints.clear();

                let diagnostics = client.diagnostics.get_buffer_diagnostics(buffer_handle);
                diagnostics.clear();

                for diagnostic in params.diagnostics.elements(&client.json) {
                    let diagnostic = DocumentDiagnostic::from_json(diagnostic, &client.json)?;
                    let range = diagnostic.range.into_buffer_range();

                    lints.add(diagnostic.message.as_str(&client.json), range);
                    diagnostics.add(range.from, &diagnostic.data, &client.json);
                }

                diagnostics.sort();
            }

            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn on_response(
    client: &mut Client,
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    response: ServerResponse,
) -> Result<(), ProtocolError> {
    let method = match client.pending_requests.take(response.id) {
        Some(method) => method,
        None => return Ok(()),
    };

    client.write_to_log_file(|buf, json| {
        use io::Write;
        let _ = write!(
            buf,
            "receive response\nid: {}\nmethod: '{}'\n",
            response.id.0, method
        );
        match &response.result {
            Ok(result) => {
                let _ = buf.write_all(b"result:\n");
                let _ = json.write(buf, result);
            }
            Err(error) => {
                let _ = write!(
                    buf,
                    "error_code: {}\nerror_message: '{}'\nerror_data:\n",
                    error.code,
                    error.message.as_str(json)
                );
                let _ = json.write(buf, &error.data);
            }
        }
    });

    let result = match response.result {
        Ok(result) => result,
        Err(error) => {
            client.request_state = RequestState::Idle;
            util::write_response_error(&mut ctx.editor.status_bar, error, &client.json);
            return Ok(());
        }
    };

    match method {
        "initialize" => {
            let mut server_name = "";
            for (key, value) in result.members(&client.json) {
                match key {
                    "capabilities" => {
                        client.server_capabilities =
                            ServerCapabilities::from_json(value, &client.json)?
                    }
                    "serverInfo" => {
                        if let JsonValue::String(name) = value.get("name", &client.json) {
                            server_name = name.as_str(&client.json);
                        }
                    }
                    _ => (),
                }
            }

            match server_name {
                "" => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Info)
                    .str("lsp server started"),
                _ => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Info)
                    .fmt(format_args!("lsp server '{}' started", server_name)),
            }

            client.initialized = true;
            client.notify(&mut ctx.platform, "initialized", JsonObject::default());

            for buffer in ctx.editor.buffers.iter() {
                util::send_did_open(client, &ctx.editor, &mut ctx.platform, buffer.handle());
            }

            Ok(())
        }
        "textDocument/hover" => {
            let contents = result.get("contents", &client.json);
            let info = util::extract_markup_content(contents, &client.json);
            ctx.editor.status_bar.write(MessageKind::Info).str(info);
            Ok(())
        }
        "textDocument/signatureHelp" => {
            #[derive(Default)]
            struct SignatureHelp {
                active_signature: usize,
                signatures: JsonArray,
            }
            impl<'json> FromJson<'json> for SignatureHelp {
                fn from_json(
                    value: JsonValue,
                    json: &'json Json,
                ) -> Result<Self, JsonConvertError> {
                    let mut this = Self::default();
                    for (key, value) in value.members(json) {
                        match key {
                            "activeSignature" => {
                                this.active_signature = usize::from_json(value, json)?;
                            }
                            "signatures" => {
                                this.signatures = JsonArray::from_json(value, json)?;
                            }
                            _ => (),
                        }
                    }
                    Ok(this)
                }
            }

            #[derive(Default)]
            struct SignatureInformation<'a> {
                label: JsonString,
                documentation: &'a str,
            }
            impl<'json> FromJson<'json> for SignatureInformation<'json> {
                fn from_json(
                    value: JsonValue,
                    json: &'json Json,
                ) -> Result<Self, JsonConvertError> {
                    let mut this = Self::default();
                    for (key, value) in value.members(json) {
                        match key {
                            "label" => this.label = JsonString::from_json(value, json)?,
                            "documentation" => {
                                this.documentation = util::extract_markup_content(value, json);
                            }
                            _ => (),
                        }
                    }
                    Ok(this)
                }
            }

            let signature_help: Option<SignatureHelp> = FromJson::from_json(result, &client.json)?;
            let signature = match signature_help.and_then(|sh| {
                sh.signatures
                    .elements(&client.json)
                    .nth(sh.active_signature)
            }) {
                Some(signature) => signature,
                None => return Ok(()),
            };
            let signature = SignatureInformation::from_json(signature, &client.json)?;
            let label = signature.label.as_str(&client.json);

            if signature.documentation.is_empty() {
                ctx.editor.status_bar.write(MessageKind::Info).str(label);
            } else {
                ctx.editor
                    .status_bar
                    .write(MessageKind::Info)
                    .fmt(format_args!("{}\n{}", signature.documentation, label));
            }

            Ok(())
        }
        "textDocument/definition" => {
            let client_handle = match client.request_state {
                RequestState::Definition { client_handle } => client_handle,
                _ => return Ok(()),
            };
            goto_definition(client, ctx, plugin_handle, client_handle, result)
        }
        "textDocument/declaration" => {
            let client_handle = match client.request_state {
                RequestState::Declaration { client_handle } => client_handle,
                _ => return Ok(()),
            };
            goto_definition(client, ctx, plugin_handle, client_handle, result)
        }
        "textDocument/implementation" => {
            let client_handle = match client.request_state {
                RequestState::Implementation { client_handle } => client_handle,
                _ => return Ok(()),
            };
            goto_definition(client, ctx, plugin_handle, client_handle, result)
        }
        "textDocument/references" => {
            let (client_handle, context_len) = match client.request_state {
                RequestState::References {
                    client_handle,
                    context_len,
                } => (client_handle, context_len),
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;
            let locations = match result {
                JsonValue::Array(locations) => locations,
                _ => return Ok(()),
            };

            let mut buffer_name = ctx.editor.string_pool.acquire();
            for location in locations.clone().elements(&client.json) {
                let location = DocumentLocation::from_json(location, &client.json)?;
                let Uri::Path(path) = Uri::parse(&client.root, location.uri.as_str(&client.json))?;

                if let Some(buffer) = ctx
                    .editor
                    .buffers
                    .find_with_path(&ctx.editor.current_directory, path)
                    .map(|h| ctx.editor.buffers.get(h))
                {
                    let range = location.range.into_buffer_range();
                    for text in buffer.content().text_range(range) {
                        buffer_name.push_str(text);
                    }
                    break;
                }
            }
            if buffer_name.is_empty() {
                buffer_name.push_str("lsp");
            }
            buffer_name.push_str(".refs");

            let buffer_view_handle = ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(&buffer_name),
                BufferProperties::scratch(),
                true,
            );
            ctx.editor.string_pool.release(buffer_name);
            let buffer_view_handle = match buffer_view_handle {
                Ok(handle) => handle,
                Err(error) => {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("{}", error));
                    return Ok(());
                }
            };

            let mut count = 0;
            let mut context_buffer = BufferContent::new();

            let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
            let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);

            let range = BufferRange::between(BufferPosition::zero(), buffer.content().end());
            buffer.delete_range(&mut ctx.editor.word_database, range, &mut ctx.editor.events);

            buffer.properties = BufferProperties::scratch();

            let mut text = ctx.editor.string_pool.acquire();
            let mut last_path = "";
            for location in locations.elements(&client.json) {
                let location = match DocumentLocation::from_json(location, &client.json) {
                    Ok(location) => location,
                    Err(_) => continue,
                };
                let path = match Uri::parse(&client.root, location.uri.as_str(&client.json)) {
                    Ok(Uri::Path(path)) => path,
                    Err(_) => continue,
                };
                let path = match path.to_str() {
                    Some(path) => path,
                    None => continue,
                };

                use fmt::Write;
                let position = location.range.start.into_buffer_position();
                let _ = writeln!(
                    text,
                    "{}:{},{}",
                    path,
                    position.line_index + 1,
                    position.column_byte_index + 1,
                );

                if context_len > 0 {
                    if last_path != path {
                        context_buffer.clear();
                        if let Ok(file) = File::open(path) {
                            let mut reader = io::BufReader::new(file);
                            let _ = context_buffer.read(&mut reader);
                        }
                    }

                    let line_count = context_buffer.lines().len();
                    let start = line_count
                        .min((location.range.start.line as usize).saturating_sub(context_len - 1));
                    let end = line_count.min(location.range.end.line as usize + context_len);

                    for line in &context_buffer.lines()[start..end] {
                        let line = line.as_str();
                        if line.is_empty() {
                            text.push('~');
                        } else {
                            text.push_str(line);
                        }
                        text.push('\n');
                    }
                    text.push('\n');
                }

                let position = buffer.content().end();
                buffer.insert_text(
                    &mut ctx.editor.word_database,
                    position,
                    &text,
                    &mut ctx.editor.events,
                );
                text.clear();

                count += 1;
                last_path = path;
            }

            if count == 1 {
                text.push_str("1 reference found\n");
            } else {
                use fmt::Write;
                let _ = writeln!(text, "{} references found\n", count);
            }

            buffer.insert_text(
                &mut ctx.editor.word_database,
                BufferPosition::zero(),
                &text,
                &mut ctx.editor.events,
            );
            ctx.editor.string_pool.release(text);

            let client = ctx.clients.get_mut(client_handle);
            client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);

            let mut cursors = ctx
                .editor
                .buffer_views
                .get_mut(buffer_view_handle)
                .cursors
                .mut_guard();
            cursors.clear();
            cursors.add(Cursor {
                anchor: BufferPosition::zero(),
                position: BufferPosition::zero(),
            });

            Ok(())
        }
        "textDocument/prepareRename" => {
            let (buffer_handle, buffer_position) = match client.request_state {
                RequestState::Rename {
                    buffer_handle,
                    buffer_position,
                } => (buffer_handle, buffer_position),
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;
            let result = match result {
                JsonValue::Null => {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Error)
                        .str("could not rename item under cursor");
                    return Ok(());
                }
                JsonValue::Object(result) => result,
                _ => return Ok(()),
            };
            let mut range = DocumentRange::default();
            let mut placeholder: Option<JsonString> = None;
            let mut default_behaviour: Option<bool> = None;
            for (key, value) in result.members(&client.json) {
                match key {
                    "start" => range.start = DocumentPosition::from_json(value, &client.json)?,
                    "end" => range.end = DocumentPosition::from_json(value, &client.json)?,
                    "range" => range = DocumentRange::from_json(value, &client.json)?,
                    "placeholder" => placeholder = FromJson::from_json(value, &client.json)?,
                    "defaultBehavior" => {
                        default_behaviour = FromJson::from_json(value, &client.json)?
                    }
                    _ => (),
                }
            }

            let buffer = ctx.editor.buffers.get(buffer_handle);

            let mut range = range.into_buffer_range();
            if let Some(true) = default_behaviour {
                let word = buffer.content().word_at(buffer_position);
                range = BufferRange::between(word.position, word.end_position());
            }

            let mut input = ctx.editor.string_pool.acquire();
            match placeholder {
                Some(text) => input.push_str(text.as_str(&client.json)),
                None => {
                    for text in buffer.content().text_range(range) {
                        input.push_str(text);
                    }
                }
            }

            read_line::enter_rename_mode(ctx, plugin_handle, &input, client);

            ctx.editor.string_pool.release(input);

            client.request_state = RequestState::FinishRename {
                buffer_handle,
                buffer_position,
            };
            Ok(())
        }
        "textDocument/rename" => {
            if let JsonValue::Null = result {
                ctx.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .str("could not rename item under cursor");
                return Ok(());
            }

            let edit = WorkspaceEdit::from_json(result, &client.json)?;
            edit.apply(
                &mut ctx.editor,
                &mut client.temp_edits,
                &client.root,
                &client.json,
            );
            Ok(())
        }
        "textDocument/codeAction" => {
            match client.request_state {
                RequestState::CodeAction => (),
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;
            let actions = match result {
                JsonValue::Array(actions) => actions,
                _ => return Ok(()),
            };

            ctx.editor.picker.clear();
            for action in actions
                .clone()
                .elements(&client.json)
                .filter_map(|a| DocumentCodeAction::from_json(a, &client.json).ok())
                .filter(|a| !a.disabled)
            {
                ctx.editor
                    .picker
                    .add_custom_entry(action.title.as_str(&client.json));
            }

            picker::enter_code_action_mode(ctx, plugin_handle, client);

            client.request_state = RequestState::FinishCodeAction;
            client.request_raw_json.clear();
            let _ = client
                .json
                .write(&mut client.request_raw_json, &actions.into());

            Ok(())
        }
        "textDocument/documentSymbol" => {
            let buffer_view_handle = match client.request_state {
                RequestState::DocumentSymbols { buffer_view_handle } => buffer_view_handle,
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;
            let symbols = match result {
                JsonValue::Array(symbols) => symbols,
                _ => return Ok(()),
            };

            fn add_symbols(picker: &mut Picker, depth: usize, symbols: JsonArray, json: &Json) {
                let indent_buf = [b' '; 32];
                let indent_len = indent_buf.len().min(depth * 2);

                for symbol in symbols
                    .elements(json)
                    .filter_map(|s| DocumentSymbolInformation::from_json(s, json).ok())
                {
                    let indent =
                        unsafe { std::str::from_utf8_unchecked(&indent_buf[..indent_len]) };

                    let name = symbol.name.as_str(json);
                    match symbol.container_name {
                        Some(container_name) => {
                            let container_name = container_name.as_str(json);
                            picker.add_custom_entry_fmt(format_args!(
                                "{}{} ({})",
                                indent, name, container_name,
                            ));
                        }
                        None => picker.add_custom_entry_fmt(format_args!("{}{}", indent, name,)),
                    }

                    add_symbols(picker, depth + 1, symbol.children.clone(), json);
                }
            }

            ctx.editor.picker.clear();
            add_symbols(&mut ctx.editor.picker, 0, symbols.clone(), &client.json);

            picker::enter_document_symbol_mode(ctx, plugin_handle, client);

            client.request_state = RequestState::FinishDocumentSymbols { buffer_view_handle };
            client.request_raw_json.clear();
            let _ = client
                .json
                .write(&mut client.request_raw_json, &symbols.into());

            Ok(())
        }
        "workspace/symbol" => {
            match client.request_state {
                RequestState::WorkspaceSymbols => (),
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;
            let symbols = match result {
                JsonValue::Array(symbols) => symbols,
                _ => return Ok(()),
            };

            ctx.editor.picker.clear();
            for symbol in symbols
                .clone()
                .elements(&client.json)
                .filter_map(|s| DocumentSymbolInformation::from_json(s, &client.json).ok())
            {
                let name = symbol.name.as_str(&client.json);
                match symbol.container_name {
                    Some(container_name) => {
                        let container_name = container_name.as_str(&client.json);
                        ctx.editor
                            .picker
                            .add_custom_entry_fmt(format_args!("{} ({})", name, container_name,));
                    }
                    None => ctx.editor.picker.add_custom_entry(name),
                }
            }

            picker::enter_workspace_symbol_mode(ctx, plugin_handle, client);

            client.request_state = RequestState::FinishWorkspaceSymbols;
            client.request_raw_json.clear();
            let _ = client
                .json
                .write(&mut client.request_raw_json, &symbols.into());

            Ok(())
        }
        "textDocument/formatting" => {
            let buffer_handle = match client.request_state {
                RequestState::Formatting { buffer_handle } => buffer_handle,
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;
            let edits = match result {
                JsonValue::Array(edits) => edits,
                _ => return Ok(()),
            };

            TextEdit::apply_edits(
                &mut ctx.editor,
                buffer_handle,
                &mut client.temp_edits,
                edits,
                &client.json,
            );

            for buffer_view in ctx.editor.buffer_views.iter() {
                let position = buffer_view.cursors.main_cursor().position;
                ctx.editor.events.enqueue_fix_cursors(
                    buffer_view.handle(),
                    &[Cursor {
                        anchor: position,
                        position,
                    }],
                );
            }

            Ok(())
        }
        "textDocument/completion" => {
            let (client_handle, buffer_handle) = match client.request_state {
                RequestState::Completion {
                    client_handle,
                    buffer_handle,
                } => (client_handle, buffer_handle),
                _ => return Ok(()),
            };
            client.request_state = RequestState::Idle;

            if ctx.editor.mode.kind() != ModeKind::Insert {
                return Ok(());
            }

            let buffer_view_handle = match ctx.clients.get(client_handle).buffer_view_handle() {
                Some(handle) => handle,
                None => return Ok(()),
            };
            let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
            if buffer_view.buffer_handle != buffer_handle {
                return Ok(());
            }
            let buffer = ctx.editor.buffers.get(buffer_handle).content();

            let completions = match result {
                JsonValue::Array(completions) => completions,
                JsonValue::Object(completions) => match completions.get("items", &client.json) {
                    JsonValue::Array(completions) => completions,
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            };

            ctx.editor.picker.clear();
            for completion in completions.elements(&client.json) {
                if let Ok(completion) = DocumentCompletionItem::from_json(completion, &client.json)
                {
                    let text = completion.text.as_str(&client.json);
                    ctx.editor.picker.add_custom_entry(text);
                }
            }

            let position = buffer_view.cursors.main_cursor().position;
            let position = buffer.position_before(position);
            let word = buffer.word_at(position);
            let filter = match word.kind {
                WordKind::Identifier => word.text,
                _ => "",
            };
            ctx.editor
                .picker
                .filter_completion(ctx.editor.word_database.word_indices(), filter);

            Ok(())
        }
        _ => Ok(()),
    }
}

fn goto_definition(
    client: &mut Client,
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    client_handle: client::ClientHandle,
    result: JsonValue,
) -> Result<(), ProtocolError> {
    enum DefinitionLocation {
        Single(DocumentLocation),
        Many(JsonArray),
        Invalid,
    }
    impl DefinitionLocation {
        pub fn parse(value: JsonValue, json: &Json) -> Self {
            match value {
                JsonValue::Object(_) => match DocumentLocation::from_json(value, json) {
                    Ok(location) => Self::Single(location),
                    Err(_) => Self::Invalid,
                },
                JsonValue::Array(array) => {
                    let mut locations = array
                        .clone()
                        .elements(json)
                        .filter_map(move |l| DocumentLocation::from_json(l, json).ok());
                    let location = match locations.next() {
                        Some(location) => location,
                        None => return Self::Invalid,
                    };
                    match locations.next() {
                        Some(_) => Self::Many(array),
                        None => Self::Single(location),
                    }
                }
                _ => Self::Invalid,
            }
        }
    }

    client.request_state = RequestState::Idle;
    match DefinitionLocation::parse(result, &client.json) {
        DefinitionLocation::Single(location) => {
            let Uri::Path(path) = Uri::parse(&client.root, location.uri.as_str(&client.json))?;

            match ctx.editor.buffer_view_handle_from_path(
                client_handle,
                path,
                BufferProperties::text(),
                false,
            ) {
                Ok(buffer_view_handle) => {
                    let client = ctx.clients.get_mut(client_handle);
                    client
                        .set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);

                    let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
                    let position = location.range.start.into_buffer_position();
                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }
                Err(error) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error)),
            }

            Ok(())
        }
        DefinitionLocation::Many(locations) => {
            ctx.editor.picker.clear();
            for location in locations
                .elements(&client.json)
                .filter_map(|l| DocumentLocation::from_json(l, &client.json).ok())
            {
                let path = match Uri::parse(&client.root, location.uri.as_str(&client.json)) {
                    Ok(Uri::Path(path)) => path,
                    Err(_) => continue,
                };
                let path = match path.to_str() {
                    Some(path) => path,
                    None => continue,
                };

                let position = location.range.start.into_buffer_position();
                ctx.editor.picker.add_custom_entry_fmt(format_args!(
                    "{}:{},{}",
                    path,
                    position.line_index + 1,
                    position.column_byte_index + 1
                ));
            }

            picker::enter_definition_mode(ctx, plugin_handle, client);
            Ok(())
        }
        DefinitionLocation::Invalid => Ok(()),
    }
}

