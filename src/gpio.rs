use crate::ble::gatt::GATTServer;
use defmt::info;
use embassy_nrf::gpio::{AnyPin, Input, Pin as _, Pull};
use nrf_softdevice::ble::Connection;

pub async fn button_task(
    n: usize,
    pin: &mut Input<'static, AnyPin>,
    gatt: &GATTServer,
    con: &Connection,
) {
    loop {
        pin.wait_for_low().await;
        info!("Button {:?} pressed!", n);
        gatt.hid
            .report
            .value_notify(con, &[0x1, 0x0, 0x4, 0x0, 0x0, 0x0, 0x0, 0x0]);
        pin.wait_for_high().await;
        gatt.hid
            .report
            .value_notify(con, &[0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0]);
        info!("Button {:?} released!", n);
    }
}
