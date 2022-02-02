use crate::async_engine::{AsyncFd, SpawnedFuture};
use crate::client::objects::Objects;
use crate::ifs::wl_buffer::{WlBuffer, WlBufferId};
use crate::ifs::wl_callback::WlCallback;
use crate::ifs::wl_compositor::WlCompositorObj;
use crate::ifs::wl_data_device::WlDataDevice;
use crate::ifs::wl_data_device_manager::WlDataDeviceManagerObj;
use crate::ifs::wl_data_offer::WlDataOffer;
use crate::ifs::wl_data_source::WlDataSource;
use crate::ifs::wl_display::WlDisplay;
use crate::ifs::wl_drm::WlDrmObj;
use crate::ifs::wl_output::WlOutputObj;
use crate::ifs::wl_region::{WlRegion, WlRegionId};
use crate::ifs::wl_registry::{WlRegistry, WlRegistryId};
use crate::ifs::wl_seat::wl_keyboard::WlKeyboard;
use crate::ifs::wl_seat::wl_pointer::WlPointer;
use crate::ifs::wl_seat::wl_touch::WlTouch;
use crate::ifs::wl_seat::{WlSeatId, WlSeatObj};
use crate::ifs::wl_shm::WlShmObj;
use crate::ifs::wl_shm_pool::WlShmPool;
use crate::ifs::wl_subcompositor::WlSubcompositorObj;
use crate::ifs::wl_surface::wl_subsurface::WlSubsurface;
use crate::ifs::wl_surface::xdg_surface::xdg_popup::XdgPopup;
use crate::ifs::wl_surface::xdg_surface::xdg_toplevel::{XdgToplevel, XdgToplevelId};
use crate::ifs::wl_surface::xdg_surface::{XdgSurface, XdgSurfaceId};
use crate::ifs::wl_surface::{WlSurface, WlSurfaceId};
use crate::ifs::xdg_positioner::{XdgPositioner, XdgPositionerId};
use crate::ifs::xdg_wm_base::XdgWmBaseObj;
use crate::ifs::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1;
use crate::ifs::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1Obj;
use crate::ifs::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1Obj;
use crate::ifs::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1;
use crate::object::{Object, ObjectId, WL_DISPLAY_ID};
use crate::state::State;
use crate::utils::buffd::{MsgFormatter, MsgParser, MsgParserError};
use crate::utils::numcell::NumCell;
use crate::utils::oneshot::{oneshot, OneshotTx};
use crate::utils::queue::AsyncQueue;
use crate::ErrorFmt;
use ahash::AHashMap;
pub use error::ClientError;
use std::cell::{Cell, RefCell, RefMut};
use std::fmt::{Debug, Display, Formatter};
use std::mem;
use std::rc::Rc;
use uapi::{c, OwnedFd};

mod error;
mod objects;
mod tasks;

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
    fn should_log(&self) -> bool {
        true
    }
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

    pub fn get_xdg_toplevel(&self, id: XdgToplevelId) -> Result<Rc<XdgToplevel>, ClientError> {
        match self.objects.xdg_toplevel.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::XdgToplevelDoesNotExist(id)),
        }
    }

    pub fn get_xdg_positioner(
        &self,
        id: XdgPositionerId,
    ) -> Result<Rc<XdgPositioner>, ClientError> {
        match self.objects.xdg_positioners.get(&id) {
            Some(r) => Ok(r),
            _ => Err(ClientError::XdgPositionerDoesNotExist(id)),
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
        if !event.should_log() {
            return;
        }
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
simple_add_obj!(ZxdgToplevelDecorationV1);
simple_add_obj!(ZxdgDecorationManagerV1Obj);

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
dedicated_add_obj!(XdgPositioner, xdg_positioners);
dedicated_add_obj!(XdgToplevel, xdg_toplevel);