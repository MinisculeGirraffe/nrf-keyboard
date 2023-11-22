use super::{
    bonder::{Bonder, KnownPeers},
    gatt::GATTServer,
    BATTERY_SERVICE, DEVICE_INFO_SERVICE, HID_SERVICE,
};
use crate::kvstore::{DBReadError, KVStore, SerdeDB};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use defmt::{error, info};
use embassy_executor::Spawner;
use nrf_softdevice::{
    ble::{
        self,
        peripheral::{self, AdvertiseError},
        Address, Connection, IdentityKey, IdentityResolutionKey,
    },
    raw, Softdevice,
};
use nrf_softdevice_s140::*;
use serde::{Deserialize, Serialize};

#[embassy_executor::task]
pub async fn softdevice_task(sd: &'static Softdevice) -> ! {
    sd.run().await
}

fn softdevice_config() -> nrf_softdevice::Config {
    nrf_softdevice::Config {
        clock: Some(raw::nrf_clock_lf_cfg_t {
            source: raw::NRF_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: 16,
            rc_temp_ctiv: 2,
            accuracy: raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
        }),
        conn_gap: Some(raw::ble_gap_conn_cfg_t {
            conn_count: 1,
            event_length: 24,
        }),
        conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 1024 }),
        gatts_attr_tab_size: Some(raw::ble_gatts_cfg_attr_tab_size_t {
            attr_tab_size: raw::BLE_GATTS_ATTR_TAB_SIZE_DEFAULT,
        }),
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 3,
            central_role_count: 3,
            central_sec_count: 3,
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            write_perm: raw::ble_gap_conn_sec_mode_t {
                _bitfield_1: raw::ble_gap_conn_sec_mode_t::new_bitfield_1(1, 2), // TODO: Change this!
            },
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
            p_value: b"Rust Keyboard" as *const u8 as _,
            current_len: 13,
            max_len: 13,
        }),
        ..Default::default()
    }
}

pub async fn sync_peers(sd: &Softdevice, db: &'static KVStore) {
    let known_peers: Result<KnownPeers, DBReadError> = db.read(KnownPeers::KEY).await;

    if let Err(ref e) = known_peers {
        error!("{}", e);
        match e {
            DBReadError::IO(ref e) => match e {
                ekv::ReadError::KeyNotFound => {
                    let mut wtx = db.write_transaction().await;
                    db.write(KnownPeers::KEY, &KnownPeers::default(), &mut wtx)
                        .await;
                    wtx.commit().await.expect("Failed to commit");
                }
                _ => panic!("DB Error"),
            },
            _ => {}
        }
    }

    let known_peers = known_peers.unwrap_or_default();
    info!("Known Peers: {}", known_peers);
    let id_keys: Vec<IdentityKey> = known_peers
        .iter()
        .filter_map(|i| *i)
        .map(|peer| peer.peer_id)
        .collect();

    let irks: Vec<IdentityResolutionKey> = known_peers
        .iter()
        .filter_map(|i| *i)
        .map(|i| i.peer_id.irk)
        .collect();
    // ble::set_device_identities_list(&sd, id_keys.as_slice(), Some(irks.as_slice())).unwrap();

    let addrs: Vec<Address> = known_peers
        .iter()
        .filter_map(|i| *i)
        .map(|p| p.peer_id.addr)
        .collect();
    //ble::set_whitelist(&sd, addrs.as_slice()).expect("Failed");
}

pub async fn init(
    spawner: Spawner,
    db: &'static KVStore,
) -> (&'static Softdevice, GATTServer, Bonder, AdvData) {
    let adv_data: AdvData = db.read(AdvData::KEY).await.unwrap_or_default();
    let config = softdevice_config();
    let sd = Softdevice::enable(&config);

    let server = GATTServer::new(sd).expect("failed to create GATT server");
    sync_peers(&sd, db).await;
    let known_peers: KnownPeers = db.read(KnownPeers::KEY).await.expect("failed");
    info!("Known Peers {}", known_peers);
    let bonder = Bonder::new(known_peers);

    bonder.spawn_task(spawner, db);
    spawner.must_spawn(softdevice_task(sd));

    (sd, server, bonder, adv_data)
}

pub async fn advertise(
    sd: &Softdevice,
    adv: &AdvData,
    bonder: &'static Bonder,
) -> Result<Connection, AdvertiseError> {
    let config = peripheral::Config::default();
    let adv = adv.to_bytes();
    let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
        adv_data: adv.as_slice(),
        scan_data: &[],
    };

    info!("Advertising Started");
    peripheral::advertise_pairable(sd, adv, &config, bonder).await
}

/// https://infocenter.nordicsemi.com/topic/com.nordic.infocenter.s140.api.v7.3.0/group___b_l_e___g_a_p___a_d___t_y_p_e___d_e_f_i_n_i_t_i_o_n_s.html?cp=5_7_4_1_2_1_1_5
/// https://bitbucket.org/bluetooth-SIG/public/src/main/assigned_numbers/
#[derive(Debug, Serialize, Deserialize)]
pub struct AdvData {
    pub flags: u8,
    pub uuids: Vec<u16>,
    pub name: String,
    pub appearance: u8,
}

impl Default for AdvData {
    fn default() -> Self {
        AdvData {
            flags: raw::BLE_GAP_ADV_FLAGS_LE_ONLY_GENERAL_DISC_MODE as u8,
            name: "HelloWorld".to_string(),
            uuids: [HID_SERVICE, DEVICE_INFO_SERVICE, BATTERY_SERVICE].to_vec(),
            appearance: BLE_APPEARANCE_HID_KEYBOARD as u8,
        }
    }
}

impl AdvData {
    pub const KEY: &'static [u8] = b"AdvData";
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(2);
        data.push(BLE_GAP_AD_TYPE_APPEARANCE as u8);
        data.push(self.appearance);

        //flags
        data.extend_from_slice(&[0x02, BLE_GAP_AD_TYPE_FLAGS as u8, self.flags]);

        //service uuids
        data.push(self.uuids.len() as u8 * 2 + 1);
        data.push(BLE_GAP_AD_TYPE_16BIT_SERVICE_UUID_COMPLETE as u8);
        self.uuids
            .iter()
            .for_each(|uuid| data.extend_from_slice(&uuid.to_le_bytes()));

        //name
        data.push(self.name.len() as u8 + 1);
        data.push(BLE_GAP_AD_TYPE_COMPLETE_LOCAL_NAME as u8);
        data.extend_from_slice(self.name.as_bytes());

        data
    }
}
