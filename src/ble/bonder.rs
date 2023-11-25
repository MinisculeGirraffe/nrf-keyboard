use crate::kvstore::{DBKey, DBWriteError, KVStore, SerdeDB};
use core::cell::{OnceCell, RefCell};
use core::ops::{Deref, DerefMut};
use defmt::{debug, info, unwrap, Format};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::{Channel, Receiver};
use nrf_softdevice::ble::gatt_server::get_sys_attrs;
use nrf_softdevice::ble::Address;
use nrf_softdevice::ble::{
    gatt_server::set_sys_attrs,
    security::{IoCapabilities, SecurityHandler},
    Connection, EncryptionInfo, IdentityKey, MasterId,
};
use serde::{Deserialize, Serialize};
use static_cell::StaticCell;
use tinyvec::ArrayVec;

#[derive(Debug, Clone, Copy, Format)]

pub struct Peer {
    pub master_id: MasterId,
    pub key: EncryptionInfo,
    pub peer_id: IdentityKey,
    pub sys_attrs: SysAttrs,
}
impl DBKey for Peer {
    fn key(&self) -> &[u8] {
        &self.peer_id.addr.bytes
    }
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct SysAttrs(ArrayVec<[u8; 64]>);

impl Deref for SysAttrs {
    type Target = ArrayVec<[u8; 64]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Format for SysAttrs {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "SysAttrs  {=[u8]:#X}", &self)
    }
}

impl DerefMut for SysAttrs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
type BonderChannel = Channel<NoopRawMutex, BonderMessage, 3>;
type BonderReceiver = Receiver<'static, NoopRawMutex, BonderMessage, 3>;
pub const BONDER: OnceCell<Bonder> = OnceCell::new();
static CHANNEL: StaticCell<BonderChannel> = StaticCell::new();
pub struct Bonder {
    peer: RefCell<Option<Peer>>,
    pub known_peers: RefCell<KnownPeers>,
    channel: &'static BonderChannel,
}
impl Bonder {
    pub fn new(known_peers: KnownPeers) -> Self {
        let channel = CHANNEL.init(BonderChannel::new());
        Self {
            peer: RefCell::new(None),
            known_peers: RefCell::new(known_peers),
            channel,
        }
    }
    pub fn spawn_task(&self, spawner: Spawner, db: &'static KVStore) {
        let recv = self.channel.receiver();
        spawner.must_spawn(bonder_task(recv, db))
    }
}

impl SecurityHandler for Bonder {
    fn io_capabilities(&self) -> IoCapabilities {
        IoCapabilities::DisplayOnly // TODO Change to KeyboardOnly
    }

    fn can_bond(&self, _conn: &Connection) -> bool {
        true
    }

    fn display_passkey(&self, passkey: &[u8; 6]) {
        info!("The passkey is \"{:a}\"", passkey)
    }

    fn on_bonded(
        &self,
        _conn: &Connection,
        master_id: MasterId,
        key: EncryptionInfo,
        peer_id: IdentityKey,
    ) {
        info!("storing bond for: id: {}, key: {}", master_id, key);
        let peer = Peer {
            master_id,
            key,
            peer_id,
            sys_attrs: SysAttrs(ArrayVec::new()),
        };

        self.known_peers.borrow_mut().add_peer(peer);
        self.channel
            .try_send(BonderMessage::Store(*self.known_peers.borrow()))
            .unwrap();
        self.peer.replace(Some(peer));
    }

    fn get_key(&self, _conn: &Connection, master_id: MasterId) -> Option<EncryptionInfo> {
        info!("getting bond for: id: {}", master_id);

        let key = self
            .known_peers
            .borrow()
            .iter()
            .filter_map(|i| *i)
            .find(|peer| peer.master_id == master_id)
            .map(|peer| peer.key);
        info!("bond: {}", key);

        key
    }

    fn save_sys_attrs(&self, conn: &Connection) {
        info!("saving system attributes for: {}", conn.peer_address());

        for peer_ref in self.known_peers.borrow_mut().iter_mut() {
            if peer_ref.is_some() {
                let mut peer = peer_ref.unwrap();
                if peer.peer_id.is_match(conn.peer_address()) {
                    let capacity = peer.sys_attrs.capacity();
                    peer.sys_attrs.resize(capacity, 0);
                    let len = get_sys_attrs(conn, &mut peer.sys_attrs).unwrap();
                    peer.sys_attrs.truncate(len);
                    *peer_ref = Some(peer);
                    break;
                }
            }
        }
        self.channel
            .try_send(BonderMessage::Store(*self.known_peers.borrow()))
            .unwrap();
    }

    fn load_sys_attrs(&self, conn: &Connection) {
        let addr = conn.peer_address();
        info!("loading system attributes for: {}", addr);

        let attrs = self
            .known_peers
            .borrow()
            .iter()
            .filter_map(|i| *i)
            .find(|peer| peer.peer_id.is_match(addr))
            .filter(|i| i.sys_attrs.is_empty())
            .map(|i| i.sys_attrs);

        unwrap!(match attrs {
            Some(attrs) => set_sys_attrs(conn, Some(attrs.as_slice())),
            None => set_sys_attrs(conn, None),
        });
    }
}

#[derive(Debug, Format)]
enum BonderMessage {
    Store(KnownPeers),
}

#[derive(Debug, Clone, Copy, Default, Format)]
pub struct KnownPeers([Option<Peer>; 3]);

impl KnownPeers {
    pub const KEY: &'static [u8] = b"knownpeers";

    fn known_peer(&self, addr: Address) -> bool {
        self.iter()
            .any(|i| i.is_some_and(|i| i.peer_id.is_match(addr)))
    }
    /// Return true if the value was inserted sucessfullt
    /// Returns false if there is no space in the known peers
    fn add_peer(&mut self, peer: Peer) -> bool {
        let addr = peer.peer_id.addr;

        if self.known_peer(addr) {
            return true;
        }

        for id in self.iter_mut() {
            if id.is_none() {
                *id = Some(peer);
                return true;
            }
        }
        return false;
    }

    fn remove_peer(&mut self, peer: Peer) {
        let addr = peer.peer_id.addr;
        for id in self.iter_mut() {
            if id.is_some_and(|i| i.peer_id.is_match(addr)) {
                *id = None;
                return;
            }
        }
    }
}

impl Deref for KnownPeers {
    type Target = [Option<Peer>; 3];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for KnownPeers {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[embassy_executor::task]
pub async fn bonder_task(recv: BonderReceiver, db: &'static KVStore) {
    loop {
        let msg = recv.receive().await;
        info!("Got Bonder Message: {}", msg);
        let result = match msg {
            BonderMessage::Store(peers) => {

                /*   let mut wtx = db.write_transaction().await;
                db.write(KnownPeers::KEY, &peers, &mut wtx)
                    .await
                    .expect("Failed to write");
                wtx.commit().await.expect("Failed to write")

                */
            }
        };
    }
}
