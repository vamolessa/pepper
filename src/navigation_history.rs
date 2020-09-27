use std::path::{Path, PathBuf};

use crate::{
    buffer::BufferCollection,
    buffer_position::BufferPosition,
    buffer_view::{BufferView, BufferViewCollection},
    client::Client,
};

pub struct NavigationHistoryPositionRef<'a> {
    buffer_path: &'a Path,
    position: BufferPosition,
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
        self.current_index = self.positions.len();

        self.positions.push(NavigationHistoryPosition {
            buffer_path_index,
            position,
        })
    }

    pub fn navigate_backward(&mut self) -> Option<NavigationHistoryPositionRef> {
        if self.current_index == 0 {
            return None;
        }

        let position = &self.positions[self.current_index];
        self.current_index -= 1;

        Some(NavigationHistoryPositionRef {
            buffer_path: &self.buffer_paths[position.buffer_path_index],
            position: position.position,
        })
    }

    pub fn navigate_forward(&mut self) -> Option<NavigationHistoryPositionRef> {
        if self.current_index == self.positions.len() - 1 {
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
