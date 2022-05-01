use {
    crate::{
        it::{
            test_error::TestError, test_object::TestObject, test_transport::TestTransport,
            testrun::ParseFull,
        },
        object::ObjectId,
        utils::buffd::MsgParser,
        wire::{wl_display::*, WlDisplayId},
    },
    std::rc::Rc,
};

pub struct TestDisplay {
    pub transport: Rc<TestTransport>,
    pub id: WlDisplayId,
}

impl TestDisplay {
    fn handle_error(&self, parser: MsgParser<'_, '_>) -> Result<(), TestError> {
        let ev = Error::parse_full(parser)?;
        let msg = format!("Compositor sent an error: {}", ev.message);
        self.transport.error(&msg);
        self.transport.kill();
        Ok(())
    }

    fn handle_delete_id(&self, parser: MsgParser<'_, '_>) -> Result<(), TestError> {
        let ev = DeleteId::parse_full(parser)?;
        match self.transport.objects.remove(&ObjectId::from_raw(ev.id)) {
            None => {
                let msg = format!(
                    "Compositor sent delete_id for object {} which does not exist",
                    ev.id
                );
                self.transport.error(&msg);
                self.transport.kill();
            }
            Some(obj) => {
                obj.on_remove(&self.transport);
                self.transport.obj_ids.borrow_mut().release(ev.id);
            }
        }
        Ok(())
    }
}

test_object! {
    TestDisplay, WlDisplay;

    ERROR => handle_error,
    DELETE_ID => handle_delete_id,
}

impl TestObject for TestDisplay {}