use std::path::{Path, PathBuf};

use crate::{buffer::BufferCollection, buffer_position::BufferPosition, buffer_view::BufferView};

pub struct NavigationHistoryPositionRef<'a> {
    pub buffer_path: &'a Path,
    pub position: BufferPosition,
}

struct NavigationHistoryPosition {
    buffer_path_index: usize,
    position: BufferPosition,
}

#[derive(Default)]
pub struct NavigationHistory {
    buffer_paths: Vec<PathBuf>,
    positions: Vec<NavigationHistoryPosition>,
    current_index: usize,
}

impl NavigationHistory {
    pub fn add_position(&mut self, buffer_path: &Path, position: BufferPosition) {
        let buffer_path_index = match self.buffer_paths.iter().position(|p| p == buffer_path) {
            Some(index) => index,
            None => {
                let index = self.buffer_paths.len();
                self.buffer_paths.push(buffer_path.into());
                index
            }
        };

        self.positions.truncate(self.current_index);

        if let Some(last) = self.positions.last() {
            if last.buffer_path_index == buffer_path_index && last.position == position {
                return;
            }
        }

        self.positions.push(NavigationHistoryPosition {
            buffer_path_index,
            position,
        });
        self.current_index = self.positions.len();
    }

    pub fn add_position_from_buffer_view(
        &mut self,
        buffer_view: &BufferView,
        buffers: &BufferCollection,
    ) {
        if let Some(buffer_path) = buffers
            .get(buffer_view.buffer_handle)
            .and_then(|b| b.path())
        {
            let main_cursor_position = buffer_view.cursors.main_cursor().position;
            self.add_position(buffer_path, main_cursor_position);
        }
    }

    pub fn navigate_backward(&mut self) -> Option<NavigationHistoryPositionRef> {
        if self.current_index == 0 {
            return None;
        }

        self.current_index -= 1;
        let position = &self.positions[self.current_index];

        Some(NavigationHistoryPositionRef {
            buffer_path: &self.buffer_paths[position.buffer_path_index],
            position: position.position,
        })
    }

    pub fn navigate_forward(&mut self) -> Option<NavigationHistoryPositionRef> {
        if self.current_index == self.positions.len() {
            return None;
        }

        let position = &self.positions[self.current_index];
        self.current_index += 1;

        Some(NavigationHistoryPositionRef {
            buffer_path: &self.buffer_paths[position.buffer_path_index],
            position: position.position,
        })
    }
}
