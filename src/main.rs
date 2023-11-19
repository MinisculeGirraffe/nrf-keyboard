#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(generic_const_exprs)]
#![feature(error_in_core)]

pub mod ble;
pub mod kvstore;
extern crate alloc;

use alloc::string::String;
use ble::{gatt::GATTServer, softdevice};
use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_nrf::{
    self as _, bind_interrupts,
    interrupt::{Interrupt, InterruptExt, Priority},
    peripherals::{self},
    qspi::{self, Frequency, Qspi},
};
use embassy_time::Timer;
use embedded_alloc::Heap;
use kvstore::init_db;
use nrf_softdevice::{self as _, Softdevice};
use panic_probe as _;
use serde::{Deserialize, Serialize};

#[global_allocator]
static HEAP: Heap = Heap::empty();

bind_interrupts!(struct QSPIIRQ {
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    init_heap();
    let qspi = init_peripherials();
    let (server, sd) = softdevice::init(spawner);

    let mut buf = [0u8; 4];
    let _ = nrf_softdevice::random_bytes(sd, &mut buf);

    let db = init_db(qspi).await;

    //  let data: AdvData = db.read_key().await.expect("Failed to read config");
    // info!("Device Name {}", data.name.as_str());

    init_bt(spawner, sd, server).await;
}
#[derive(Debug, Deserialize, Serialize)]
struct DeviceConfig {
    advertising_name: String,
    appearance: u8,
}

fn init_heap() {
    use core::mem::MaybeUninit;
    const HEAP_SIZE: usize = 1024 * 30;
    static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
    info!("Heap Initalized: Size: {}", HEAP_SIZE);
}

fn init_peripherials<'a>() -> Qspi<'a, embassy_nrf::peripherals::QSPI> {
    Interrupt::RNG.set_priority(Priority::P3);
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;

    let p = embassy_nrf::init(config);

    info!("Peripherals Initalized");
    Interrupt::RNG.set_priority(Priority::P3);

    //let rng = embassy_nrf::rng::Rng::new(p.RNG, RNGIRQ);

    let mut config = qspi::Config::default();

    config.capacity = 64 * 1024 * 1024; // 64MB
    config.frequency = Frequency::M32;
    config.read_opcode = qspi::ReadOpcode::READ4IO;
    config.write_opcode = qspi::WriteOpcode::PP4IO;
    config.write_page_size = qspi::WritePageSize::_256BYTES;

    Interrupt::QSPI.set_priority(Priority::P3);
    let qspi = qspi::Qspi::new(
        p.QSPI, QSPIIRQ, p.P1_03, p.P1_06, p.P1_05, p.P1_04, p.P1_02, p.P1_01, config,
    );

    qspi
}

async fn init_bt(spawner: Spawner, sd: &Softdevice, server: GATTServer) {
    info!("Softdevice initialized");
    info!("Server: {}", server);
    let con = softdevice::advertise(&sd)
        .await
        .expect("failed to advertise");
    info!("Advertising Completed");

    spawner.must_spawn(crate::ble::gatt_task(con.clone(), server.clone()));
    Timer::after_secs(30).await;
    let mut buf = [0u8; 8];
    loop {
        buf[2] = 0x0;
        info!(
            "Sent Report: {=[u8]:#X} - Status :{}",
            buf,
            server.hid.report.value_notify(&con, &buf)
        );
        Timer::after_millis(500).await;

        buf[2] = 0x04;

        info!(
            "Sent Report: {=[u8]:#X} - Status :{}",
            buf,
            server.hid.report.value_notify(&con, &buf)
        );
    }
}
