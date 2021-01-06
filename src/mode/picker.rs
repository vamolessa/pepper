use crate::{
    client_event::Key,
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    word_database::EmptyWordCollection,
};

pub struct State {
    on_client_keys: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll),
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _| (),
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.read_line.set_input("");
        ctx.picker.filter(&EmptyWordCollection, "");
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.read_line.set_input("");
        ctx.picker.reset();
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let this = &mut ctx.mode.picker_state;
        match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next() {
                    Key::Ctrl('n') | Key::Ctrl('j') | Key::Down => ctx.picker.move_cursor(1),
                    Key::Ctrl('p') | Key::Ctrl('k') | Key::Up => ctx.picker.move_cursor(-1),
                    Key::Ctrl('d') | Key::PageDown => {
                        let picker_height = ctx
                            .picker
                            .height(ctx.config.values.picker_max_height.get() as _)
                            as isize;
                        ctx.picker.move_cursor(picker_height / 2);
                    }
                    Key::Ctrl('u') | Key::PageUp => {
                        let picker_height = ctx
                            .picker
                            .height(ctx.config.values.picker_max_height.get() as _)
                            as isize;
                        ctx.picker.move_cursor(-picker_height / 2);
                    }
                    Key::Ctrl('b') | Key::Home => {
                        let cursor = ctx.picker.cursor() as isize;
                        ctx.picker.move_cursor(-cursor);
                    }
                    Key::Ctrl('e') | Key::End => {
                        let cursor = ctx.picker.cursor() as isize;
                        let entry_count = ctx.picker.height(isize::MAX as _) as isize;
                        ctx.picker.move_cursor(entry_count - cursor - 1);
                    }
                    _ => ctx
                        .picker
                        .filter(&EmptyWordCollection, ctx.read_line.input()),
                }

                (this.on_client_keys)(ctx, keys, ReadLinePoll::Pending);
            }
            poll => (this.on_client_keys)(ctx, keys, poll),
        }

        ModeOperation::None
    }
}

pub mod buffer {
    use super::*;

    use std::path::Path;

    use crate::{buffer::Buffer, navigation_history::NavigationHistory, picker::Picker};

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            match poll {
                ReadLinePoll::Pending => return,
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    return;
                }
            }

            let path = match ctx.picker.current_entry(&EmptyWordCollection) {
                Some(entry) => entry.name,
                None => {
                    Mode::change_to(ctx, ModeKind::default());
                    return;
                }
            };

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                ctx.buffer_views,
                ctx.target_client,
            );

            match ctx.buffer_views.buffer_view_handle_from_path(
                ctx.buffers,
                ctx.word_database,
                ctx.target_client,
                ctx.current_directory,
                Path::new(path),
                None,
                ctx.editor_events,
            ) {
                Ok(handle) => ctx.set_current_buffer_view_handle(Some(handle)),
                Err(error) => ctx.status_message.write_error(&error),
            }

            Mode::change_to(ctx, ModeKind::default());
        }

        fn add_buffer_to_picker(picker: &mut Picker, buffer: &Buffer) {
            if let Some(path) = buffer.path().and_then(|p| p.to_str()) {
                picker.add_custom_entry(path, if buffer.needs_save() { "changed" } else { "" });
            }
        }

        ctx.read_line.set_prompt("buffer:");
        ctx.picker.reset();

        let buffers = &ctx.buffers;
        let buffer_views = &ctx.buffer_views;
        let prevous_buffer_handle = ctx
            .clients
            .get(ctx.target_client)
            .and_then(|c| c.previous_buffer_view_handle())
            .and_then(|h| buffer_views.get(h))
            .map(|v| v.buffer_handle);

        if let Some(buffer) = prevous_buffer_handle.and_then(|h| buffers.get(h)) {
            add_buffer_to_picker(ctx.picker, buffer);
        }

        for buffer in ctx.buffers.iter() {
            let buffer_handle = buffer.handle();
            if prevous_buffer_handle
                .map(|h| h != buffer_handle)
                .unwrap_or(true)
            {
                add_buffer_to_picker(ctx.picker, buffer);
            }
        }

        ctx.mode.picker_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::Picker);
    }
}

pub mod custom {
    use super::*;

    use crate::script::{ScriptCallback, ScriptContext, ScriptValue};

    pub fn enter_mode(ctx: &mut ScriptContext, callback: ScriptCallback) {
        fn on_client_keys(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            let previous_mode_kind = ctx.mode.kind();
            let (engine, mut script_ctx) = ctx.into_script_context();
            let result = engine.as_ref_with_ctx(&mut script_ctx, |engine, ctx, guard| {
                let (name, description) = match poll {
                    ReadLinePoll::Pending => return Ok(()),
                    ReadLinePoll::Submitted => {
                        match ctx.picker.current_entry(&EmptyWordCollection) {
                            Some(entry) => (
                                ScriptValue::String(engine.create_string(entry.name.as_bytes())?),
                                ScriptValue::String(
                                    engine.create_string(entry.description.as_bytes())?,
                                ),
                            ),
                            None => (ScriptValue::Nil, ScriptValue::Nil),
                        }
                    }
                    ReadLinePoll::Canceled => (ScriptValue::Nil, ScriptValue::Nil),
                };

                if let Some(callback) = ctx.script_callbacks.picker.take() {
                    callback.call(engine, &guard, (name, description))?;
                    callback.dispose(engine)?;
                }

                Ok(())
            });

            match result {
                Ok(()) => {
                    if ctx.mode.kind() == previous_mode_kind {
                        Mode::change_to(ctx, ModeKind::default());
                    }
                }
                Err(error) => {
                    ctx.status_message.write_error(&error);
                    Mode::change_to(ctx, ModeKind::default());
                }
            }
        }

        ctx.script_callbacks.picker = Some(callback);
        ctx.mode.picker_state.on_client_keys = on_client_keys;
        //Mode::change_to(ctx, ModeKind::Picker);
    }
}
