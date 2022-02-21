use std::rc::Rc;
use thiserror::Error;
use crate::client::{Client, ClientError};
use crate::ifs::wl_output::{SEND_DONE_SINCE, WlOutput};
use crate::leaks::Tracker;
use crate::object::Object;
use crate::utils::buffd::{MsgParser, MsgParserError};
use crate::wire::ZxdgOutputV1Id;
use crate::wire::zxdg_output_v1::*;

pub const NAME_SINCE: u32 = 2;
pub const DESCRIPTION_SINCE: u32 = 2;
pub const NO_DONE_SINCE: u32 = 3;

pub struct ZxdgOutputV1 {
    pub id: ZxdgOutputV1Id,
    pub version: u32,
    pub client: Rc<Client>,
    pub output: Rc<WlOutput>,
    pub tracker: Tracker<Self>,
}

impl ZxdgOutputV1 {
    pub fn send_logical_position(&self, x: i32, y: i32) {
        self.client.event(LogicalPosition {
            self_id: self.id,
            x,
            y,
        });
    }

    pub fn send_logical_size(&self, width: i32, height: i32) {
        self.client.event(LogicalSize {
            self_id: self.id,
            width,
            height,
        });
    }

    pub fn send_done(&self) {
        self.client.event(Done {
            self_id: self.id,
        });
    }

    pub fn send_name(&self, name: &str) {
        self.client.event(Name {
            self_id: self.id,
            name,
        });
    }

    pub fn send_description(&self, description: &str) {
        self.client.event(Description {
            self_id: self.id,
            description,
        });
    }

    pub fn send_updates(&self) {
        let pos = self.output.global.position();
        self.send_logical_position(pos.x1(), pos.y1());
        self.send_logical_size(pos.width(), pos.height());
        if self.version >= NO_DONE_SINCE || self.output.version < SEND_DONE_SINCE {
            self.output.send_done();
        } else {
            self.send_done();
        }
    }

    pub fn destroy(&self, msg: MsgParser) -> Result<(), DestroyError> {
        let _req: Destroy = self.client.parse(self, msg)?;
        self.output.xdg_outputs.remove(&self.id);
        self.client.remove_obj(self)?;
        Ok(())
    }
}

object_base! {
    ZxdgOutputV1, ZxdgOutputV1Error;

    DESTROY => destroy,
}

impl Object for ZxdgOutputV1 {
    fn num_requests(&self) -> u32 {
        DESTROY + 1
    }
}

simple_add_obj!(ZxdgOutputV1);

#[derive(Debug, Error)]
pub enum ZxdgOutputV1Error {
    #[error("Could not process a `destroy` request")]
    DestroyError(#[from] DestroyError),
}

#[derive(Debug, Error)]
pub enum DestroyError {
    #[error("Parsing failed")]
    MsgParserError(#[source] Box<MsgParserError>),
    #[error(transparent)]
    ClientError(Box<ClientError>),
}
efrom!(DestroyError, MsgParserError);
efrom!(DestroyError, ClientError);