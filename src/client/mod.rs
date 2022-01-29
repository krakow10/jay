use crate::async_engine::{AsyncError, AsyncFd, SpawnedFuture};
use crate::client::objects::Objects;
use crate::ifs::org_kde_kwin_server_decoration::{
    OrgKdeKwinServerDecoration, OrgKdeKwinServerDecorationError,
};
use crate::ifs::org_kde_kwin_server_decoration_manager::{
    OrgKdeKwinServerDecorationManagerError, OrgKdeKwinServerDecorationManagerObj,
};
use crate::ifs::wl_buffer::{WlBuffer, WlBufferError, WlBufferId};
use crate::ifs::wl_callback::WlCallback;
use crate::ifs::wl_compositor::{WlCompositorError, WlCompositorObj};
use crate::ifs::wl_data_device::{WlDataDevice, WlDataDeviceError};
use crate::ifs::wl_data_device_manager::{WlDataDeviceManagerError, WlDataDeviceManagerObj};
use crate::ifs::wl_data_offer::{WlDataOffer, WlDataOfferError};
use crate::ifs::wl_data_source::{WlDataSource, WlDataSourceError};
use crate::ifs::wl_display::{WlDisplay, WlDisplayError};
use crate::ifs::wl_drm::{WlDrmError, WlDrmObj};
use crate::ifs::wl_output::{WlOutputError, WlOutputObj};
use crate::ifs::wl_region::{WlRegion, WlRegionError, WlRegionId};
use crate::ifs::wl_registry::{WlRegistry, WlRegistryError, WlRegistryId};
use crate::ifs::wl_seat::wl_keyboard::{WlKeyboard, WlKeyboardError};
use crate::ifs::wl_seat::wl_pointer::{WlPointer, WlPointerError};
use crate::ifs::wl_seat::wl_touch::{WlTouch, WlTouchError};
use crate::ifs::wl_seat::{WlSeatError, WlSeatId, WlSeatObj};
use crate::ifs::wl_shm::{WlShmError, WlShmObj};
use crate::ifs::wl_shm_pool::{WlShmPool, WlShmPoolError};
use crate::ifs::wl_subcompositor::{WlSubcompositorError, WlSubcompositorObj};
use crate::ifs::wl_surface::wl_subsurface::{WlSubsurface, WlSubsurfaceError};
use crate::ifs::wl_surface::xdg_surface::xdg_popup::{XdgPopup, XdgPopupError};
use crate::ifs::wl_surface::xdg_surface::xdg_toplevel::{XdgToplevel, XdgToplevelError};
use crate::ifs::wl_surface::xdg_surface::{XdgSurface, XdgSurfaceError, XdgSurfaceId};
use crate::ifs::wl_surface::{WlSurface, WlSurfaceError, WlSurfaceId};
use crate::ifs::xdg_positioner::{XdgPositioner, XdgPositionerError};
use crate::ifs::xdg_wm_base::{XdgWmBaseError, XdgWmBaseObj};
use crate::ifs::zwp_linux_buffer_params_v1::{ZwpLinuxBufferParamsV1, ZwpLinuxBufferParamsV1Error};
use crate::ifs::zwp_linux_dmabuf_v1::{ZwpLinuxDmabufV1Error, ZwpLinuxDmabufV1Obj};
use crate::object::{Object, ObjectId, WL_DISPLAY_ID};
use crate::state::State;
use crate::utils::buffd::{BufFdError, MsgFormatter, MsgParser, MsgParserError};
use crate::utils::numcell::NumCell;
use crate::utils::oneshot::{oneshot, OneshotTx};
use crate::utils::queue::AsyncQueue;
use crate::ErrorFmt;
use ahash::AHashMap;
use std::cell::{Cell, RefCell, RefMut};
use std::fmt::{Debug, Display, Formatter};
use std::mem;
use std::rc::Rc;
use thiserror::Error;
use uapi::{c, OwnedFd};

mod objects;
mod tasks;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("An error occurred in the async engine")]
    Async(#[from] AsyncError),
    #[error("An error occurred reading from/writing to the client")]
    Io(#[from] BufFdError),
    #[error("An error occurred while processing a request")]
    RequestError(#[source] Box<ClientError>),
    #[error("Client tried to invoke a non-existent method")]
    InvalidMethod,
    #[error("Client tried to access non-existent object {0}")]
    InvalidObject(ObjectId),
    #[error("The message size is < 8")]
    MessageSizeTooSmall,
    #[error("The size of the message is not a multiple of 4")]
    UnalignedMessage,
    #[error("The requested client {0} does not exist")]
    ClientDoesNotExist(ClientId),
    #[error("There is no wl_region with id {0}")]
    RegionDoesNotExist(WlRegionId),
    #[error("There is no wl_buffer with id {0}")]
    BufferDoesNotExist(WlBufferId),
    #[error("There is no wl_surface with id {0}")]
    SurfaceDoesNotExist(WlSurfaceId),
    #[error("There is no xdg_surface with id {0}")]
    XdgSurfaceDoesNotExist(XdgSurfaceId),
    #[error("There is no wl_seat with id {0}")]
    WlSeatDoesNotExist(WlSeatId),
    #[error("Cannot parse the message")]
    ParserError(#[source] Box<MsgParserError>),
    #[error("Server tried to allocate more than 0x1_00_00_00 ids")]
    TooManyIds,
    #[error("The server object id is out of bounds")]
    ServerIdOutOfBounds,
    #[error("The object id is unknown")]
    UnknownId,
    #[error("The id is already in use")]
    IdAlreadyInUse,
    #[error("The client object id is out of bounds")]
    ClientIdOutOfBounds,
    #[error("An error occurred in a `wl_display`")]
    WlDisplayError(#[source] Box<WlDisplayError>),
    #[error("An error occurred in a `wl_registry`")]
    WlRegistryError(#[source] Box<WlRegistryError>),
    #[error("Could not add object {0} to the client")]
    AddObjectError(ObjectId, #[source] Box<ClientError>),
    #[error("An error occurred in a `wl_surface`")]
    WlSurfaceError(#[source] Box<WlSurfaceError>),
    #[error("An error occurred in a `wl_compositor`")]
    WlCompositorError(#[source] Box<WlCompositorError>),
    #[error("An error occurred in a `wl_shm`")]
    WlShmError(#[source] Box<WlShmError>),
    #[error("An error occurred in a `wl_shm_pool`")]
    WlShmPoolError(#[source] Box<WlShmPoolError>),
    #[error("An error occurred in a `wl_region`")]
    WlRegionError(#[source] Box<WlRegionError>),
    #[error("An error occurred in a `wl_subsurface`")]
    WlSubsurfaceError(#[source] Box<WlSubsurfaceError>),
    #[error("An error occurred in a `wl_subcompositor`")]
    WlSubcompositorError(#[source] Box<WlSubcompositorError>),
    #[error("An error occurred in a `xdg_surface`")]
    XdgSurfaceError(#[source] Box<XdgSurfaceError>),
    #[error("An error occurred in a `xdg_positioner`")]
    XdgPositionerError(#[source] Box<XdgPositionerError>),
    #[error("An error occurred in a `xdg_popup`")]
    XdgPopupError(#[source] Box<XdgPopupError>),
    #[error("An error occurred in a `xdg_toplevel`")]
    XdgToplevelError(#[source] Box<XdgToplevelError>),
    #[error("An error occurred in a `xdg_wm_base`")]
    XdgWmBaseError(#[source] Box<XdgWmBaseError>),
    #[error("An error occurred in a `wl_buffer`")]
    WlBufferError(#[source] Box<WlBufferError>),
    #[error("An error occurred in a `wl_output`")]
    WlOutputError(#[source] Box<WlOutputError>),
    #[error("An error occurred in a `wl_seat`")]
    WlSeatError(#[source] Box<WlSeatError>),
    #[error("An error occurred in a `wl_pointer`")]
    WlPointerError(#[source] Box<WlPointerError>),
    #[error("An error occurred in a `wl_keyboard`")]
    WlKeyboardError(#[source] Box<WlKeyboardError>),
    #[error("An error occurred in a `wl_touch`")]
    WlTouchError(#[source] Box<WlTouchError>),
    #[error("Object {0} is not a display")]
    NotADisplay(ObjectId),
    #[error("An error occurred in a `wl_data_device`")]
    WlDataDeviceError(#[source] Box<WlDataDeviceError>),
    #[error("An error occurred in a `wl_data_device_manager`")]
    WlDataDeviceManagerError(#[source] Box<WlDataDeviceManagerError>),
    #[error("An error occurred in a `wl_data_offer`")]
    WlDataOfferError(#[source] Box<WlDataOfferError>),
    #[error("An error occurred in a `wl_data_source`")]
    WlDataSourceError(#[source] Box<WlDataSourceError>),
    #[error("An error occurred in a `zwp_linx_dmabuf_v1`")]
    ZwpLinuxDmabufV1Error(#[source] Box<ZwpLinuxDmabufV1Error>),
    #[error("An error occurred in a `zwp_linx_buffer_params_v1`")]
    ZwpLinuxBufferParamsV1Error(#[source] Box<ZwpLinuxBufferParamsV1Error>),
    #[error("An error occurred in a `wl_drm`")]
    WlDrmError(#[source] Box<WlDrmError>),
    #[error("An error occurred in a `org_kde_kwin_server_decoration_manager`")]
    OrgKdeKwinServerDecorationManagerError(#[source] Box<OrgKdeKwinServerDecorationManagerError>),
    #[error("An error occurred in a `org_kde_kwin_server_decoration`")]
    OrgKdeKwinServerDecorationError(#[source] Box<OrgKdeKwinServerDecorationError>),
}

efrom!(ClientError, ParserError, MsgParserError);
efrom!(ClientError, WlDisplayError);
efrom!(ClientError, WlRegistryError);
efrom!(ClientError, WlSurfaceError);
efrom!(ClientError, WlCompositorError);
efrom!(ClientError, WlShmError);
efrom!(ClientError, WlShmPoolError);
efrom!(ClientError, WlRegionError);
efrom!(ClientError, WlSubsurfaceError);
efrom!(ClientError, WlSubcompositorError);
efrom!(ClientError, XdgSurfaceError);
efrom!(ClientError, XdgPositionerError);
efrom!(ClientError, XdgWmBaseError);
efrom!(ClientError, XdgToplevelError);
efrom!(ClientError, XdgPopupError);
efrom!(ClientError, WlBufferError);
efrom!(ClientError, WlOutputError);
efrom!(ClientError, WlSeatError);
efrom!(ClientError, WlTouchError);
efrom!(ClientError, WlPointerError);
efrom!(ClientError, WlKeyboardError);
efrom!(
    ClientError,
    WlDataDeviceManagerError,
    WlDataDeviceManagerError
);
efrom!(ClientError, WlDataDeviceError);
efrom!(ClientError, WlDataSourceError);
efrom!(ClientError, WlDataOfferError);
efrom!(ClientError, ZwpLinuxDmabufV1Error);
efrom!(
    ClientError,
    ZwpLinuxBufferParamsV1Error,
    ZwpLinuxBufferParamsV1Error
);
efrom!(ClientError, WlDrmError, WlDrmError);
efrom!(
    ClientError,
    OrgKdeKwinServerDecorationManagerError,
    OrgKdeKwinServerDecorationManagerError
);
efrom!(
    ClientError,
    OrgKdeKwinServerDecorationError,
    OrgKdeKwinServerDecorationError
);

impl ClientError {
    fn peer_closed(&self) -> bool {
        matches!(self, ClientError::Io(BufFdError::Closed))
    }
}

#[derive(Debug, Copy, Clone, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub struct ClientId(u64);

impl Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

pub struct Clients {
    next_client_id: NumCell<u64>,
    pub clients: RefCell<AHashMap<ClientId, ClientHolder>>,
    shutdown_clients: RefCell<AHashMap<ClientId, ClientHolder>>,
}

impl Clients {
    pub fn new() -> Self {
        Self {
            next_client_id: NumCell::new(1),
            clients: Default::default(),
            shutdown_clients: Default::default(),
        }
    }

    pub fn id(&self) -> ClientId {
        ClientId(self.next_client_id.fetch_add(1))
    }

    #[allow(dead_code)]
    pub fn get(&self, id: ClientId) -> Result<Rc<Client>, ClientError> {
        let clients = self.clients.borrow();
        match clients.get(&id) {
            Some(c) => Ok(c.data.clone()),
            _ => Err(ClientError::ClientDoesNotExist(id)),
        }
    }

    pub fn spawn(
        &self,
        id: ClientId,
        global: &Rc<State>,
        socket: OwnedFd,
    ) -> Result<(), ClientError> {
        let (uid, pid) = {
            let mut cred = c::ucred {
                pid: 0,
                uid: 0,
                gid: 0,
            };
            match uapi::getsockopt(socket.raw(), c::SOL_SOCKET, c::SO_PEERCRED, &mut cred) {
                Ok(_) => (cred.uid, cred.pid),
                Err(e) => {
                    log::error!(
                        "Cannot determine peer credentials of new connection: {:?}",
                        std::io::Error::from(e)
                    );
                    return Ok(());
                }
            }
        };
        let (send, recv) = oneshot();
        let data = Rc::new(Client {
            id,
            state: global.clone(),
            checking_queue_size: Cell::new(false),
            socket: global.eng.fd(&Rc::new(socket))?,
            objects: Objects::new(),
            events: AsyncQueue::new(),
            shutdown: Cell::new(Some(send)),
            shutdown_sent: Cell::new(false),
            dispatch_frame_requests: AsyncQueue::new(),
        });
        let display = Rc::new(WlDisplay::new(&data));
        data.objects.display.set(Some(display.clone()));
        data.objects.add_client_object(display).expect("");
        let client = ClientHolder {
            _handler: global.eng.spawn(tasks::client(data.clone(), recv)),
            data,
        };
        log::info!(
            "Client {} connected, pid: {}, uid: {}, fd: {}",
            id,
            pid,
            uid,
            client.data.socket.raw()
        );
        self.clients.borrow_mut().insert(client.data.id, client);
        Ok(())
    }

    pub fn kill(&self, client: ClientId) {
        log::info!("Removing client {}", client.0);
        if self.clients.borrow_mut().remove(&client).is_none() {
            self.shutdown_clients.borrow_mut().remove(&client);
        }
    }

    pub fn shutdown(&self, client_id: ClientId) {
        if let Some(client) = self.clients.borrow_mut().remove(&client_id) {
            log::info!("Shutting down client {}", client.data.id.0);
            client.data.shutdown.replace(None).unwrap().send(());
            client.data.events.push(WlEvent::Shutdown);
            client.data.shutdown_sent.set(true);
            self.shutdown_clients.borrow_mut().insert(client_id, client);
        }
    }

    pub fn broadcast<B>(&self, mut f: B)
    where
        B: FnMut(&Rc<Client>),
    {
        let clients = self.clients.borrow();
        for client in clients.values() {
            f(&client.data);
        }
    }
}

impl Drop for Clients {
    fn drop(&mut self) {
        let _clients1 = mem::take(&mut *self.clients.borrow_mut());
        let _clients2 = mem::take(&mut *self.shutdown_clients.borrow_mut());
    }
}

pub struct ClientHolder {
    pub data: Rc<Client>,
    _handler: SpawnedFuture<()>,
}

impl Drop for ClientHolder {
    fn drop(&mut self) {
        self.data.objects.destroy();
        self.data.events.clear();
        self.data.dispatch_frame_requests.clear();
    }
}

pub trait EventFormatter: Debug {
    fn format(self: Box<Self>, fmt: &mut MsgFormatter<'_>);
    fn obj(&self) -> &dyn Object;
}

pub type DynEventFormatter = Box<dyn EventFormatter>;

pub trait RequestParser<'a>: Debug + Sized {
    fn parse(parser: &mut MsgParser<'_, 'a>) -> Result<Self, MsgParserError>;
}

pub enum WlEvent {
    Flush,
    Shutdown,
    Event(Box<dyn EventFormatter>),
}

pub struct Client {
    pub id: ClientId,
    pub state: Rc<State>,
    checking_queue_size: Cell<bool>,
    socket: AsyncFd,
    pub objects: Objects,
    events: AsyncQueue<WlEvent>,
    shutdown: Cell<Option<OneshotTx<()>>>,
    shutdown_sent: Cell<bool>,
    pub dispatch_frame_requests: AsyncQueue<Rc<WlCallback>>,
}

const MAX_PENDING_EVENTS: usize = 10000;

impl Client {
    pub fn invalid_request(&self, obj: &dyn Object, request: u32) {
        log::error!(
            "Client {} sent an invalid request {} on object {} of type {}",
            self.id.0,
            request,
            obj.id(),
            obj.interface().name(),
        );
        match self.display() {
            Ok(d) => self.fatal_event(d.invalid_request(obj, request)),
            Err(e) => {
                log::error!(
                    "Could not retrieve display of client {}: {}",
                    self.id,
                    ErrorFmt(e),
                );
                self.state.clients.kill(self.id);
            }
        }
    }

    pub fn new_id<T: From<ObjectId>>(&self) -> Result<T, ClientError> {
        self.objects.id(self)
    }

    pub fn display(&self) -> Result<Rc<WlDisplay>, ClientError> {
        match self.objects.display.get() {
            Some(d) => Ok(d),
            _ => Err(ClientError::NotADisplay(WL_DISPLAY_ID)),
        }
    }

    pub fn parse<'a, R: RequestParser<'a>>(
        &self,
        obj: &impl Object,
        mut parser: MsgParser<'_, 'a>,
    ) -> Result<R, MsgParserError> {
        let res = R::parse(&mut parser)?;
        parser.eof()?;
        log::trace!(
            "Client {} -> {}@{}.{:?}",
            self.id,
            obj.interface().name(),
            obj.id(),
            res
        );
        Ok(res)
    }

    pub fn protocol_error(&self, obj: &dyn Object, code: u32, message: String) {
        if let Ok(d) = self.display() {
            self.fatal_event(d.error(obj.id(), code, message));
        } else {
            self.state.clients.shutdown(self.id);
        }
    }

    pub fn fatal_event(&self, event: Box<dyn EventFormatter>) {
        self.events.push(WlEvent::Event(event));
        self.state.clients.shutdown(self.id);
    }

    pub fn event(self: &Rc<Self>, event: Box<dyn EventFormatter>) {
        self.event2(WlEvent::Event(event));
    }

    pub fn flush(self: &Rc<Self>) {
        self.event2(WlEvent::Flush);
    }

    pub fn event2(self: &Rc<Self>, event: WlEvent) {
        self.events.push(event);
        if self.events.size() > MAX_PENDING_EVENTS {
            if !self.checking_queue_size.replace(true) {
                self.state.slow_clients.push(self.clone());
            }
        }
    }

    pub async fn check_queue_size(&self) {
        if self.events.size() > MAX_PENDING_EVENTS {
            self.state.eng.yield_now().await;
            if self.events.size() > MAX_PENDING_EVENTS {
                log::error!("Client {} is too slow at fetching events", self.id.0);
                self.state.clients.kill(self.id);
                return;
            }
        }
        self.checking_queue_size.set(false);
    }

    pub fn get_buffer(&self, id: WlBufferId) -> Result<Rc<WlBuffer>, ClientError> {
        match self.objects.buffers.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::BufferDoesNotExist(id)),
        }
    }

    pub fn get_region(&self, id: WlRegionId) -> Result<Rc<WlRegion>, ClientError> {
        match self.objects.regions.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::RegionDoesNotExist(id)),
        }
    }

    pub fn get_surface(&self, id: WlSurfaceId) -> Result<Rc<WlSurface>, ClientError> {
        match self.objects.surfaces.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::SurfaceDoesNotExist(id)),
        }
    }

    pub fn get_xdg_surface(&self, id: XdgSurfaceId) -> Result<Rc<XdgSurface>, ClientError> {
        match self.objects.xdg_surfaces.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::XdgSurfaceDoesNotExist(id)),
        }
    }

    pub fn get_wl_seat(&self, id: WlSeatId) -> Result<Rc<WlSeatObj>, ClientError> {
        match self.objects.seats.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::WlSeatDoesNotExist(id)),
        }
    }

    pub fn lock_registries(&self) -> RefMut<AHashMap<WlRegistryId, Rc<WlRegistry>>> {
        self.objects.registries()
    }

    pub fn log_event(&self, event: &dyn EventFormatter) {
        let obj = event.obj();
        log::trace!(
            "Client {} <= {}@{}.{:?}",
            self.id,
            obj.interface().name(),
            obj.id(),
            event,
        );
    }

    pub fn add_client_obj<T: WaylandObject>(&self, obj: &Rc<T>) -> Result<(), ClientError> {
        self.add_obj(obj, true)
    }

    #[allow(dead_code)]
    pub fn add_server_obj<T: WaylandObject>(&self, obj: &Rc<T>) {
        self.add_obj(obj, false).expect("add_server_obj failed")
    }

    fn add_obj<T: WaylandObject>(&self, obj: &Rc<T>, client: bool) -> Result<(), ClientError> {
        if client {
            self.objects.add_client_object(obj.clone())?;
        } else {
            self.objects.add_server_object(obj.clone());
        }
        obj.clone().add(self);
        Ok(())
    }

    pub fn remove_obj<T: WaylandObject>(self: &Rc<Self>, obj: &T) -> Result<(), ClientError> {
        obj.remove(self);
        self.objects.remove_obj(self, obj.id())
    }
}

pub trait WaylandObject: Object {
    fn add(self: Rc<Self>, client: &Client) {
        let _ = client;
    }
    fn remove(&self, client: &Client) {
        let _ = client;
    }
}

macro_rules! simple_add_obj {
    ($ty:ty) => {
        impl WaylandObject for $ty {}
    };
}

simple_add_obj!(WlCompositorObj);
simple_add_obj!(WlCallback);
simple_add_obj!(WlRegistry);
simple_add_obj!(WlShmObj);
simple_add_obj!(WlShmPool);
simple_add_obj!(WlSubcompositorObj);
simple_add_obj!(WlSubsurface);
simple_add_obj!(XdgPositioner);
simple_add_obj!(XdgToplevel);
simple_add_obj!(XdgPopup);
simple_add_obj!(WlOutputObj);
simple_add_obj!(WlKeyboard);
simple_add_obj!(WlPointer);
simple_add_obj!(WlTouch);
simple_add_obj!(WlDataDeviceManagerObj);
simple_add_obj!(WlDataDevice);
simple_add_obj!(WlDataOffer);
simple_add_obj!(WlDataSource);
simple_add_obj!(ZwpLinuxDmabufV1Obj);
simple_add_obj!(ZwpLinuxBufferParamsV1);
simple_add_obj!(WlDrmObj);
simple_add_obj!(OrgKdeKwinServerDecorationManagerObj);
simple_add_obj!(OrgKdeKwinServerDecoration);

macro_rules! dedicated_add_obj {
    ($ty:ty, $field:ident) => {
        impl WaylandObject for $ty {
            fn add(self: Rc<Self>, client: &Client) {
                client.objects.$field.set(self.id().into(), self);
            }
            fn remove(&self, client: &Client) {
                client.objects.$field.remove(&self.id().into());
            }
        }
    };
}

dedicated_add_obj!(WlRegion, regions);
dedicated_add_obj!(WlSurface, surfaces);
dedicated_add_obj!(XdgWmBaseObj, xdg_wm_bases);
dedicated_add_obj!(XdgSurface, xdg_surfaces);
dedicated_add_obj!(WlBuffer, buffers);
dedicated_add_obj!(WlSeatObj, seats);
