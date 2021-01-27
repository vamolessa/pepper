use super::{Ui, UiResult};

pub struct NoneUi;
impl Ui for NoneUi {
    fn display(&mut self, _: &[u8]) -> UiResult<()> {
        Ok(())
    }
}
