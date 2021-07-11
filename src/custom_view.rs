use crate::{client::ClientManager, editor::Editor, platform::Platform, ui::RenderContext};

pub struct CustomViewUpdateContext<'a> {
    pub editor: &'a mut Editor,
    pub platform: &'a mut Platform,
    pub clients: &'a mut ClientManager,
}

pub trait CustomView {
    fn update(&mut self, ctx: &mut CustomViewUpdateContext);
    fn render(&self, ctx: &RenderContext, buf: &mut Vec<u8>);
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct CustomViewHandle(u32);

enum ViewEntry {
    Vacant,
    Reserved,
    Occupied(Box<dyn CustomView>),
}
impl ViewEntry {
    pub fn reserve_and_take(&mut self) -> Option<Box<dyn CustomView>> {
        let mut entry = Self::Reserved;
        std::mem::swap(self, &mut entry);
        match entry {
            Self::Vacant => {
                *self = Self::Vacant;
                None
            }
            Self::Reserved => None,
            Self::Occupied(view) => Some(view),
        }
    }
}

pub struct CustomViewCollection {
    entries: Vec<ViewEntry>,
}
impl CustomViewCollection {
    pub fn add(&mut self, view: Box<dyn CustomView>) -> CustomViewHandle {
        fn find_vacant_entry(this: &mut CustomViewCollection) -> CustomViewHandle {
            for (i, slot) in this.entries.iter_mut().enumerate() {
                if let ViewEntry::Vacant = slot {
                    *slot = ViewEntry::Reserved;
                    return CustomViewHandle(i as _);
                }
            }
            let handle = CustomViewHandle(this.entries.len() as _);
            this.entries.push(ViewEntry::Reserved);
            handle
        }

        let handle = find_vacant_entry(self);
        self.entries[handle.0 as usize] = ViewEntry::Occupied(view);
        handle
    }

    pub fn get(&self, handle: CustomViewHandle) -> Option<&dyn CustomView> {
        match &self.entries[handle.0 as usize] {
            ViewEntry::Occupied(view) => Some(view.as_ref()),
            _ => None,
        }
    }

    pub fn reserve_and_take(&mut self, handle: CustomViewHandle) -> Option<Box<dyn CustomView>> {
        self.entries[handle.0 as usize].reserve_and_take()
    }

    pub fn put_back(&mut self, handle: CustomViewHandle, view: Box<dyn CustomView>) {
        self.entries[handle.0 as usize] = ViewEntry::Occupied(view);
    }

    pub fn remove(&mut self, handle: CustomViewHandle) {
        self.entries[handle.0 as usize] = ViewEntry::Vacant;
    }
}

