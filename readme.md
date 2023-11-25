#Getting started

``` 
#Add the nightly toolchain
rustup toolchain install nightly
rustup component add rust-src

#add the compiler target
rustup target add thumbv7em-none-eabihf

#install probe-run
cargo install probe-rs --features cli

#Flash the soft device
probe-rs erase --chip nRF52840_xxAA 
probe-rs download --chip nRF52840_xxAA --format hex s140_nrf52_7.3.0_softdevice.hex 
```