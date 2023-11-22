#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(generic_const_exprs)]
#![feature(error_in_core)]

pub mod ble;
pub mod gpio;
pub mod kvstore;
extern crate alloc;
use alloc::string::String;
use ble::{
    bonder::Bonder,
    gatt::GATTServer,
    softdevice::{self, AdvData},
};
use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_nrf::{
    self as _, bind_interrupts,
    gpio::{AnyPin, Input, Pin},
    interrupt::{Interrupt, InterruptExt, Priority},
    peripherals::{self},
    qspi::{self, Frequency, Qspi},
};
use embassy_time::Timer;
use embedded_alloc::Heap;
use futures::future::{select, Either};
use futures::pin_mut;
use gpio::button_task;
use kvstore::{init_kvstore, KVStore};
use nrf_softdevice::{self as _, ble::gatt_server, gatt_server, Softdevice};
use panic_probe as _;
use serde::{Deserialize, Serialize};
use static_cell::StaticCell;

use crate::ble::softdevice::sync_peers;

#[global_allocator]
static HEAP: Heap = Heap::empty();

bind_interrupts!(struct QSPIIRQ {
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    init_heap();
    let (qspi, btn) = init_peripherials();
    let db = init_kvstore(qspi).await;

    let (sd, gatt, bonder, adv) = softdevice::init(spawner, db).await;

    init_bt(spawner, sd, &gatt, bonder, adv, btn, db).await;
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

fn init_peripherials<'a>() -> (
    Qspi<'a, embassy_nrf::peripherals::QSPI>,
    Input<'static, AnyPin>,
) {
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

    let btn = Input::new(p.P1_00.degrade(), embassy_nrf::gpio::Pull::Up);

    (qspi, btn)
}
static BONDER: StaticCell<Bonder> = StaticCell::new();
async fn init_bt(
    spawner: Spawner,
    sd: &'static Softdevice,
    server: &GATTServer,
    bonder: Bonder,
    adv: AdvData,
    mut btn: Input<'static, AnyPin>,
    db: &'static KVStore,
) {
    info!("Softdevice initialized");
    info!("Server: {}", server);
    let bonder = BONDER.init(bonder);

    loop {
        let con = softdevice::advertise(&sd, &adv, bonder)
            .await
            .expect("failed to advertise");

        info!("Advertising Completed");
        info!("Spawning GATT Server");

        let gatt_fut = gatt_server::run(&con, server, |f| {});
        let btn_fut = button_task(1, &mut btn, server, &con);

        pin_mut!(gatt_fut);
        pin_mut!(btn_fut);

        select(gatt_fut, btn_fut).await;
        //con.disconnect().expect("Failed to disconnect");
        info!("Gatt Server exited")
    }
}
