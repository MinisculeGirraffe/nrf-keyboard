use core::marker::PhantomData;

use defmt::{info, Format};
use nrf_softdevice::{
    ble::{
        gatt_server::{
            self,
            builder::ServiceBuilder,
            characteristic::{Attribute, Metadata, Properties},
            CharacteristicHandles, GetValueError, NotifyValueError, RegisterError, SetValueError,
            WriteOp,
        },
        Connection, SecurityMode, Uuid,
    },
    Softdevice,
};
use packed_struct::PackedStruct;
use usbd_human_interface_device::device::keyboard::{BootKeyboardReport, BOOT_KEYBOARD_REPORT_DESCRIPTOR};

#[derive(Debug, Clone, Copy, Format)]
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

    pub fn value_notify(&self, conn: &Connection, value: &T) -> Result<(), NotifyValueError> {
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
            cccd_handle: value.cccd_handle,
            sccd_handle: value.sccd_handle,
            _marker: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, Format)]
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
            Metadata::new(Properties::new().read()),
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

#[derive(Debug, Clone, Copy, Format)]
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
#[derive(Clone, Format)]
///This service exposes the HID reports and other HID data intended for HID Hosts and HID Devices. Summary: The HID Service exposes characteristics required for a HID Device to transfer HID report descriptors and reports to a HID Host. This also exposes the characteristics for a HID Host to write to a Device. The Human Interface Device Service is instantiated as a Primary Service.
pub struct HIDService {
    service_handle: u16,
    protocol_mode: CharachteristicHandle<[u8; 1]>,
    pub report: CharachteristicHandle<[u8; 8]>,
    report_map: CharachteristicHandle<[u8; 65]>,
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
            Metadata::new(Properties::new().read().write_without_response()),
        )?;

        let mut x = service_builder.add_characteristic(
            Uuid::new_16(0x2A4D),
            Attribute::new(BootKeyboardReport::default().pack().unwrap_or_default())
                .security(SecurityMode::Mitm),
            Metadata::new(Properties::new().notify().read().write()),
        )?;

        /*
                let client_characteristic_configuration = x.add_descriptor(
                    Uuid::new_16(2902),
                    Attribute::new([0x0, 0x00])
                        .security(SecurityMode::Mitm)
                        .write_security(SecurityMode::Mitm),
                )?;
        */
        let report_reference =
            x.add_descriptor(Uuid::new_16(0x2908), Attribute::new([0x0, 0x01]))?;

        let report = x.build();
                
        let mut x = service_builder.add_characteristic(
            Uuid::new_16(0x2A4B),
            Attribute::new(
                BOOT_KEYBOARD_REPORT_DESCRIPTOR,
            )
            .security(SecurityMode::Mitm),
            Metadata::new(Properties::new().read()),
        )?;
        let external_report_reference = x.add_descriptor(
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
            Metadata::new(Properties::new().write_without_response()),
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

    pub fn on_write(&self, handle: u16, data: &[u8]) {}
}
#[derive(Clone, Format)]
pub struct GATTServer {
    pub bas: BatteryService,
    pub das: DeviceInformationService,
    pub hid: HIDService,
}

impl GATTServer {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let hid = HIDService::new(sd)?;
        let bas = BatteryService::new(sd)?;
        let das = DeviceInformationService::new(sd)?;

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
        info!("Handle: {} Got Data: {=[u8]:#X}", handle, &data);
        self.bas.on_write(handle, data);
        self.hid.on_write(handle, data);
        None
    }
}
