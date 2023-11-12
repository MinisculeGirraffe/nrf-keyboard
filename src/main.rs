#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt_rtt as _;
use panic_probe as _;
use nrf_softdevice as _;
use embassy_nrf as _; // time driver

use defmt::info;
use embassy_executor::Spawner;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello from RTT!");
}