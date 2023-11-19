use crate::kvstore::{DBKey, FlashCtrl, SerdeDB};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ops::{Deref, DerefMut};
use defmt::{debug, info, unwrap};
use ekv::Database;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex};
use embassy_sync::channel::{Channel, Receiver};
use nrf_softdevice::ble::{
    gatt_server::set_sys_attrs,
    security::{IoCapabilities, SecurityHandler},
    Connection, EncryptionInfo, IdentityKey, MasterId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]

struct Peer {
    master_id: MasterId,
    key: EncryptionInfo,
    peer_id: IdentityKey,
}
impl DBKey for Peer {
    fn key(&self) -> &[u8] {
        &self.peer_id.addr.bytes
    }
}

pub struct Bonder {
    peer: Cell<Option<Peer>>,
    sys_attrs: RefCell<Vec<u8>>,
}

impl Default for Bonder {
    fn default() -> Self {
        Bonder {
            peer: Cell::new(None),
            sys_attrs: Default::default(),
        }
    }
}

impl SecurityHandler for Bonder {
    fn io_capabilities(&self) -> IoCapabilities {
        IoCapabilities::DisplayOnly
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
        debug!("storing bond for: id: {}, key: {}", master_id, key);

        // In a real application you would want to signal another task to permanently store the keys in non-volatile memory here.
        self.sys_attrs.borrow_mut().clear();
        self.peer.set(Some(Peer {
            master_id,
            key,
            peer_id,
        }));
    }

    fn get_key(&self, _conn: &Connection, master_id: MasterId) -> Option<EncryptionInfo> {
        debug!("getting bond for: id: {}", master_id);

        self.peer
            .get()
            .and_then(|peer| (master_id == peer.master_id).then_some(peer.key))
    }

    fn save_sys_attrs(&self, conn: &Connection) {
        debug!("saving system attributes for: {}", conn.peer_address());

        if let Some(peer) = self.peer.get() {
            if peer.peer_id.is_match(conn.peer_address()) {
                let mut sys_attrs = self.sys_attrs.borrow_mut();
                let capacity = sys_attrs.capacity();
                //  unwrap!(sys_attrs.resize(capacity, 0));
                //  let len = unwrap!(gatt_server::get_sys_attrs(conn, &mut sys_attrs)) as u16;
                //  sys_attrs.truncate(usize::from(len));
                // In a real application you would want to signal another task to permanently store sys_attrs for this connection's peer
            }
        }
    }

    fn load_sys_attrs(&self, conn: &Connection) {
        let addr = conn.peer_address();
        debug!("loading system attributes for: {}", addr);

        let attrs = self.sys_attrs.borrow();
        // In a real application you would search all stored peers to find a match
        let attrs = if self
            .peer
            .get()
            .map(|peer| peer.peer_id.is_match(addr))
            .unwrap_or(false)
        {
            (!attrs.is_empty()).then_some(attrs.as_slice())
        } else {
            None
        };

        unwrap!(set_sys_attrs(conn, attrs));
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct KnownPeers([[u8; 6]; 1]);

impl KnownPeers {
    const KEY: &'static [u8] = b"knownpeers";
}

impl Deref for KnownPeers {
    type Target = [[u8; 6]; 1];

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
async fn handle_writes(
    recv: Receiver<'static, ThreadModeRawMutex, BonderMessage, 3>,
    db: Database<FlashCtrl<'static>, CriticalSectionRawMutex>,
) {
    loop {
        let msg = recv.receive().await;

        match msg {
            BonderMessage::AddPeer(peer) => {
                let mut wtx = db.write_transaction().await;
                let peer_id = peer.peer_id.addr.bytes;
                let mut peer_ids: KnownPeers = db.read(KnownPeers::KEY).await.unwrap_or_default();
                // check to see if the peer is already in the list
                db.write(&peer_id, &peer, &mut wtx).await;
                let peer_known = peer_ids.iter().any(|i| i == &peer_id);
                if !peer_known {
                    for id in peer_ids.iter_mut() {
                        if id.iter().all(|&byte| byte == 0) {
                            *id = peer_id;
                            break;
                        }
                    }
                    db.write(KnownPeers::KEY, &peer_ids, &mut wtx).await;
                }
                wtx.commit().await;
            }
            BonderMessage::RemovePeer(peer) => {
                let mut wtx = db.write_transaction().await;
                let peer_id = peer.peer_id.addr.bytes;
                let mut peer_ids: KnownPeers = db.read(KnownPeers::KEY).await.unwrap_or_default();
                for id in peer_ids.iter_mut() {
                    if *id == peer_id {
                        *id = [0; 6];
                        break;
                    }
                }
                wtx.delete(&peer_id).await;
                db.write(KnownPeers::KEY, &peer_ids, &mut wtx).await;
                wtx.commit().await;
            }
        }
    }
}

enum BonderMessage {
    AddPeer(Peer),
    RemovePeer(Peer),
}
fn build_chan(spawner: Spawner) {
    let channel = Channel::<ThreadModeRawMutex, BonderMessage, 3>::new();
    let recv = channel.receiver();
    let send = channel.sender();
}
