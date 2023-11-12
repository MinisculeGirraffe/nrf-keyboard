#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
pub mod softdevice;

use defmt_rtt as _;
use embassy_nrf::{self as _, interrupt::Priority, Peripherals};
use nrf_softdevice as _;
use panic_probe as _; // time driver

use defmt::info;
use embassy_executor::Spawner;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello from RTT!");

    let p = init_peripherials();

    let sd = softdevice::init(spawner);
    info!("Softdevice initialized");
    softdevice::advertise(sd)
        .await
        .expect("failed to advertise");
    info!("Advertising Completed");
}

fn init_peripherials() -> Peripherals {
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    embassy_nrf::init(config)
}
