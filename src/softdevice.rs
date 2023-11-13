use alloc::vec::Vec;
use core::{cell::Ref, marker::PhantomData};
use defmt::info;
use embassy_executor::Spawner;
use nrf_softdevice::{
    ble::{
        gatt_server::{
            self,
            builder::ServiceBuilder,
            characteristic::{Attribute, Metadata, Properties},
            CharacteristicHandles, DescriptorHandle, GetValueError, NotifyValueError,
            RegisterError, SetValueError, WriteOp,
        },
        peripheral::{self, AdvertiseError},
        Connection, SecurityMode, Uuid,
    },
    raw, Softdevice,
};
use nrf_softdevice_s140::*;

#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice) -> ! {
    sd.run().await
}

#[embassy_executor::task]
pub async fn gatt_task(conn: Connection, server: GATTServer) {
    gatt_server::run(&conn, &server, |_| {}).await;
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
            p_value: b"HelloWorld" as *const u8 as _,
            current_len: 10,
            max_len: 10,
        }),
        ..Default::default()
    }
}

pub fn init(spawner: Spawner) -> (GATTServer, &'static Softdevice) {
    let config = softdevice_config();

    let sd = Softdevice::enable(&config);
    let server = GATTServer::new(sd).expect("failed to create GATT server");
    spawner.must_spawn(softdevice_task(sd));

    (server, sd)
}
const HID_SERVICE: u16 = 0x1812;
const BATTERY_SERVICE: u16 = 0x180F;
const DEVICE_INFO_SERVICE: u16 = 0x180A;
pub async fn advertise(sd: &Softdevice) -> Result<Connection, AdvertiseError> {
    let config = peripheral::Config::default();

    let adv = AdvData {
        flags: raw::BLE_GAP_ADV_FLAGS_LE_ONLY_GENERAL_DISC_MODE as u8,
        name: "HelloWorld",
        uuids: [HID_SERVICE, BATTERY_SERVICE, DEVICE_INFO_SERVICE],
        appearance: BLE_APPEARANCE_HID_KEYBOARD as u8,
    }
    .to_bytes();
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
#[derive(Debug, defmt::Format)]
struct AdvData<'a, const N: usize> {
    flags: u8,
    uuids: [u16; N],
    name: &'a str,
    appearance: u8,
}
impl<const N: usize> AdvData<'_, N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();

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

#[derive(Debug, Clone, Copy)]
pub struct CharachteristicHandle<T: core::convert::AsRef<[u8]> + Sized> {
    value_handle: u16,
    user_desc_handle: u16,
    cccd_handle: u16,
    sccd_handle: u16,
    _marker: PhantomData<T>,
}

impl<T> CharachteristicHandle<T>
where
    T: core::convert::AsRef<[u8]> + Sized,
    [(); core::mem::size_of::<T>()]:,
{
    pub fn new(
        sb: &mut ServiceBuilder<'_>,
        uuid: Uuid,
        attr: Attribute<T>,
        meta: Metadata,
    ) -> Result<Self, RegisterError> {
        let handles = sb.add_characteristic(uuid, attr, meta)?.build();

        Ok(handles.into())
    }

    pub fn value_get(&self, sd: &Softdevice) -> Result<T, GetValueError> {
        let mut buf = [0u8; core::mem::size_of::<T>()];
        gatt_server::get_value(sd, self.value_handle, &mut buf)?;

        let value: T = unsafe {
            // Safety: T is known by PhantomData and the size of the buffer calculated at compile time
            core::mem::transmute_copy(&buf)
        };
        Ok(value)
    }

    pub fn value_set(&self, sd: &Softdevice, value: &T) -> Result<(), SetValueError> {
        let bytes = value.as_ref();
        gatt_server::set_value(sd, self.value_handle, bytes)
    }

    pub fn value_notift(&self, conn: &Connection, value: &T) -> Result<(), NotifyValueError> {
        let bytes = value.as_ref();
        gatt_server::notify_value(conn, self.value_handle, bytes)
    }
}

impl<T> From<CharacteristicHandles> for CharachteristicHandle<T>
where
    T: core::convert::AsRef<[u8]> + Sized,
{
    fn from(value: CharacteristicHandles) -> Self {
        Self {
            value_handle: value.value_handle,
            user_desc_handle: value.user_desc_handle,
            cccd_handle: value.user_desc_handle,
            sccd_handle: value.sccd_handle,
            _marker: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BatteryService {
    service_handle: u16,
    level: CharachteristicHandle<[u8; 1]>,
}

impl BatteryService {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let mut service_builder = ServiceBuilder::new(sd, Uuid::new_16(0x180F))?;

        let level = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A19),
            Attribute::new([0u8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read().notify()),
        )?;

        Ok(BatteryService {
            service_handle: service_builder.build().handle(),
            level,
        })
    }

    pub fn on_write(&self, handle: u16, data: &[u8]) {
        if handle == self.level.cccd_handle && !data.is_empty() {
            info!("battery notifications: {}", (data[0] & 0x01) != 0);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceInformationService {
    service_handle: u16,
    manufacturer_name: CharachteristicHandle<[u8; 4]>,
    model_number: CharachteristicHandle<[u8; 1]>,
    serial_number: CharachteristicHandle<[u8; 1]>,
    hardware_revision: CharachteristicHandle<[u8; 1]>,
    firmware_revision: CharachteristicHandle<[u8; 1]>,
    ///The SYSTEM ID characteristic consists of a structure with two fields. The first field are the LSOs and the second field contains the MSOs. This is a 64-bit structure which consists of a 40-bit manufacturer-defined identifier concatenated with a 24 bit unique Organizationally Unique Identifier (OUI). The OUI is issued by the IEEE Registration Authority (http://standards.ieee.org/regauth/index.html) and is required to be used in accordance with IEEE Standard 802-2001.6 while the least significant 40 bits are manufacturer defined. If System ID generated based on a Bluetooth Device Address, it is required to be done as follows. System ID and the Bluetooth Device Address have a very similar structure: a Bluetooth Device Address is 48 bits in length and consists of a 24 bit Company Assigned Identifier (manufacturer defined identifier) concatenated with a 24 bit Company Identifier (OUI). In order to encapsulate a Bluetooth Device Address as System ID, the Company Identifier is concatenated with 0xFFFE followed by the Company Assigned Identifier of the Bluetooth Address. For more guidelines related to EUI-64, refer to http://standards.ieee.org/develop/regauth/tut/eui64.pdf. Examples: If the system ID is based of a Bluetooth Device Address with a Company Identifier (OUI) is 0x123456 and the Company Assigned Identifier is 0x9ABCDE, then the System Identifier is required to be 0x123456FFFE9ABCDE.
    system_id: CharachteristicHandle<[u8; 8]>,
    ///The PnP_ID characteristic returns its value when read using the GATT Characteristic Value Read procedure. Summary: The PnP_ID characteristic is a set of values that used to create a device ID value that is unique for this device. Included in the characteristic is a Vendor ID Source field, a Vendor ID field, a Product ID field and a Product Version field. These values are used to identify all devices of a given type/model/version using numbers.
    pnp_id: CharachteristicHandle<[u8; 7]>,
}

impl DeviceInformationService {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let mut service_builder = ServiceBuilder::new(sd, Uuid::new_16(0x180A))?;

        let manufacturer_name = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A29),
            Attribute::new([b'l', b'm', b'a', b'o']).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let model_number = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A24),
            Attribute::new([1u8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let serial_number = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A25),
            Attribute::new([1u8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let hardware_revision = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A27),
            Attribute::new([0u8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let firmware_revision = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A26),
            Attribute::new([0u8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;
        let system_id = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A23),
            Attribute::new([0u8; 8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let pnp_id = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A50),
            Attribute::new([0u8; 7]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        Ok(Self {
            service_handle: service_builder.build().handle(),
            manufacturer_name,
            model_number,
            serial_number,
            hardware_revision,
            firmware_revision,
            system_id,
            pnp_id,
        })
    }
}
#[derive(Clone)]
///This service exposes the HID reports and other HID data intended for HID Hosts and HID Devices. Summary: The HID Service exposes characteristics required for a HID Device to transfer HID report descriptors and reports to a HID Host. This also exposes the characteristics for a HID Host to write to a Device. The Human Interface Device Service is instantiated as a Primary Service.
pub struct HIDService {
    service_handle: u16,
    protocol_mode: CharachteristicHandle<[u8; 1]>,
    pub report: CharachteristicHandle<[u8; 8]>,
    report_map: CharachteristicHandle<[u8; 45]>,
    hid_information: CharachteristicHandle<[u8; 4]>,
    hid_control_point: CharachteristicHandle<[u8; 1]>,
}

impl HIDService {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let mut service_builder = ServiceBuilder::new(sd, Uuid::new_16(0x1812))?;

        let protocol_mode = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A4E),
            Attribute::new([0x01u8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let mut x = service_builder.add_characteristic(
            Uuid::new_16(0x2A4D),
            Attribute::new([0u8; 8]).security(SecurityMode::Open),
            Metadata::new(Properties::new().notify().read().write()),
        )?;

        let client_characteristic_configuration = x.add_descriptor(
            Uuid::new_128(&[
                0x00, 0x00, 0x29, 0x02, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5f, 0x9b,
                0x34, 0xfb,
            ]),
            Attribute::new([0x0, 0x01]).security(SecurityMode::Mitm),
        )?;

        let report_reference = x.add_descriptor(
            Uuid::new_16(0x2908),
            Attribute::new([0x0, 0x01]).security(SecurityMode::Mitm),
        )?;

        let report = x.build();

        let mut x = service_builder.add_characteristic(
            Uuid::new_16(0x2A4B),
            Attribute::new([
                0x05, 0x01, // Usage Page (Generic Desktop)
                0x09, 0x06, // Usage (Keyboard)
                0xa1, 0x01, // Collection (Application)
                0x05, 0x07, // Usage Page (Keyboard)
                0x19, 0xe0, // Usage Minimum (Keyboard LeftControl)
                0x29, 0xe7, // Usage Maximum (Keyboard Right GUI)
                0x15, 0x00, // Logical Minimum (0)
                0x25, 0x01, // Logical Maximum (1)
                0x75, 0x01, // Report Size (1)
                0x95, 0x08, // Report Count (8)
                0x81, 0x02, // Input (Data, Variable, Absolute) Modifier byte
                0x95, 0x01, // Report Count (1)
                0x75, 0x08, // Report Size (8)
                0x81, 0x01, // Input (Constant) Reserved byte
                0x95, 0x06, // Report Count (6)
                0x75, 0x08, // Report Size (8)
                0x15, 0x00, // Logical Minimum (0)
                0x25, 0x65, // Logical Maximum (101)
                0x05, 0x07, // Usage Page (Key Codes)
                0x05, 0x01, // Usage Minimum (Reserved (no event indicated))
                0x05, 0x01, // Usage Maximum (Keyboard Application)
                0x05, 0x01, // Input (Data,Array) Key arrays (6 bytes)
                0xc0, // End Collection
            ])
            .security(SecurityMode::Mitm),
            Metadata::new(Properties::new().read()),
        )?;
        x.add_descriptor(
            Uuid::new_16(0x2907),
            Attribute::new([0x0; 2]).security(SecurityMode::Mitm),
        )?;

        let report_map = x.build();

        let hid_information = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A4A),
            Attribute::new([0x01, 0x11, 0x00, 0x02]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        let hid_control_point = CharachteristicHandle::new(
            &mut service_builder,
            Uuid::new_16(0x2A4C),
            Attribute::new([0x0]).security(SecurityMode::Open),
            Metadata::new(Properties::new().read()),
        )?;

        Ok(Self {
            service_handle: service_builder.build().handle(),
            protocol_mode,
            report: report.into(),
            report_map: report_map.into(),
            hid_information,
            hid_control_point,
        })
    }
}
#[derive(Clone)]
pub struct GATTServer {
    pub bas: BatteryService,
    pub das: DeviceInformationService,
    pub hid: HIDService,
}

impl GATTServer {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let bas = BatteryService::new(sd)?;
        let das = DeviceInformationService::new(sd)?;
        let hid = HIDService::new(sd)?;
        Ok(Self { bas, das, hid })
    }
}

impl gatt_server::Server for GATTServer {
    type Event = ();

    fn on_write(
        &self,
        _conn: &Connection,
        handle: u16,
        _op: WriteOp,
        _offset: usize,
        data: &[u8],
    ) -> Option<Self::Event> {
        self.bas.on_write(handle, data);
        None
    }
}
