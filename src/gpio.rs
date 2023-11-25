use crate::ble::gatt::GATTServer;
use defmt::info;
use embassy_nrf::gpio::{AnyPin, Input, Pin as _, Pull};
use nrf_softdevice::ble::Connection;
use packed_struct::PackedStruct;
use usbd_human_interface_device::{device::keyboard::BootKeyboardReport, page::Keyboard};
pub async fn button_task(
    n: usize,
    pin: &mut Input<'static, AnyPin>,
    gatt: &GATTServer,
    con: &Connection,
) {
    loop {
        pin.wait_for_low().await;
        let mut report = BootKeyboardReport::default();

        report.keys[0] = Keyboard::A;
        info!("Button {:?} pressed!", n);
        gatt.hid
            .report
            .value_notify(con, &report.pack().unwrap())
            .expect("Failed to send)");

        pin.wait_for_high().await;

        gatt.hid
            .report
            .value_notify(con, &BootKeyboardReport::default().pack().unwrap())
            .expect("Failed to send)");
        //info!("Button {:?} released!", n);
    }
}

