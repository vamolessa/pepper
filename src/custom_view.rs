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

pub struct CustomViewCollection {
    views: Vec<Box<dyn CustomView>>,
}
impl CustomViewCollection {
    //
}

