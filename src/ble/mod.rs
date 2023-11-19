use nrf_softdevice::ble::{gatt_server, Connection};

use self::gatt::GATTServer;

pub mod bonder;
pub mod gatt;
pub mod softdevice;

#[embassy_executor::task]
pub async fn gatt_task(conn: Connection, server: GATTServer) {
    gatt_server::run(&conn, &server, |_| {}).await;
}

pub const HID_SERVICE: u16 = 0x1812;
pub const BATTERY_SERVICE: u16 = 0x180F;
pub const DEVICE_INFO_SERVICE: u16 = 0x180A;
