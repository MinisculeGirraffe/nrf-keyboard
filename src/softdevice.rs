use defmt::info;
use embassy_executor::Spawner;
use nrf_softdevice::{ble::{peripheral::{self, AdvertiseError}, Connection}, raw, Softdevice};
use nrf_softdevice_s140::*;
use tinyvec::ArrayVec;
use typenum::Sum;
#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice) -> ! {
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
        conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 256 }),
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 3,
            central_role_count: 3,
            central_sec_count: 3,
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            write_perm: raw::ble_gap_conn_sec_mode_t {
                _bitfield_1: raw::ble_gap_conn_sec_mode_t::new_bitfield_1(1, 1), // TODO: Change this!
            },
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
            p_value: b"HelloWorld" as *const u8 as _,
            current_len: 10,
            max_len: 10,
        }),
        ..Default::default()
    }
}

pub fn init(spawner: Spawner) -> &'static Softdevice {
    let config = softdevice_config();

    let sd = Softdevice::enable(&config);

    spawner.must_spawn(softdevice_task(sd));

    sd
}
const HID_SERVICE: u16 = 0x1812;
const BATTERY_SERVICE: u16 = 0x180F;
const DEVICE_INFO_SERVICE: u16 = 0x180A;
pub async fn advertise(sd: &Softdevice)-> Result<Connection, AdvertiseError> {
    let config = peripheral::Config::default();

    let adv = AdvData {
        flags: raw::BLE_GAP_ADV_FLAGS_LE_ONLY_GENERAL_DISC_MODE as u8,
        name: "HelloWorld",
        uuids: [HID_SERVICE, BATTERY_SERVICE, DEVICE_INFO_SERVICE],
        appearance: BLE_APPEARANCE_HID_KEYBOARD as u8,
    }.to_bytes();
   let adv_data = adv.as_slice();

   info!("Advertising: {=[u8]:#X}", &adv_data);
    let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
        adv_data,
        scan_data: &[],
    };

     peripheral::advertise_connectable(sd, adv, &config).await
}

/// https://infocenter.nordicsemi.com/topic/com.nordic.infocenter.s140.api.v7.3.0/group___b_l_e___g_a_p___a_d___t_y_p_e___d_e_f_i_n_i_t_i_o_n_s.html?cp=5_7_4_1_2_1_1_5
/// https://bitbucket.org/bluetooth-SIG/public/src/main/assigned_numbers/
#[derive(Debug,defmt::Format)]
struct AdvData<'a, const N: usize> {
    flags: u8,
    uuids: [u16; N],
    name: &'a str,
    appearance: u8,
}
impl<const N: usize> AdvData<'_, N> {
    pub fn to_bytes(&self) -> ArrayVec<[u8; 64]> {
        //TODO: check size
        let mut data: ArrayVec<[u8;64]> = ArrayVec::new();

        //flags
        data.extend_from_slice(&[0x02, BLE_GAP_AD_TYPE_FLAGS as u8, self.flags]);

        //service uuids
        data.push(N as u8 * 2 + 1);
        data.push(BLE_GAP_AD_TYPE_16BIT_SERVICE_UUID_COMPLETE as u8);
        self.uuids
            .iter()
            .for_each(|uuid| data.extend_from_slice(&uuid.to_le_bytes()));

        //name
        data.push(self.name.len() as u8 + 1);
        data.push(BLE_GAP_AD_TYPE_COMPLETE_LOCAL_NAME as u8);
        data.extend_from_slice(self.name.as_bytes());

        data.push(2);
        data.push(BLE_GAP_AD_TYPE_APPEARANCE as u8);
        data.push(self.appearance);

        data
    }
}
