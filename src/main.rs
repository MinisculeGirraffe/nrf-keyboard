#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(generic_const_exprs)]
pub mod softdevice;

extern crate alloc;

use defmt_rtt as _;
use embassy_nrf::{self as _, interrupt::Priority, Peripherals};
use nrf_softdevice as _;
use panic_probe as _; // time driver

use defmt::info;
use embassy_executor::Spawner;
use embedded_alloc::Heap;
#[global_allocator]
static HEAP: Heap = Heap::empty();
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    init_heap();
    let _ = init_peripherials();

    let (server, sd) = softdevice::init(spawner);
    info!("Softdevice initialized");
    let con = softdevice::advertise(&sd)
        .await
        .expect("failed to advertise");
    info!("Advertising Completed");

    spawner.must_spawn(softdevice::gatt_task(con.clone(), server.clone()));
    info!("GATT Server Spawned");
    loop {
        let mut report = [0x0u8; 8];
        report[2] = 0x04;

        match server.hid.report.value_notift(&con, &report) {
            Err(e) => {
                info!("{}", e);
            }
            _ => {}
        }
        info!("Wrote report");
    }
}

fn init_heap() {
    use core::mem::MaybeUninit;
    const HEAP_SIZE: usize = 1024 * 100;
    static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
    info!("Heap Initalized: Size: {}", HEAP_SIZE);
}

fn init_peripherials() -> Peripherals {
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(config);

    info!("Peripherals Initalized");
    p
}
