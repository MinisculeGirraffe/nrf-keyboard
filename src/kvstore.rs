use defmt::{info, unwrap};
use ekv::{CommitError, Database, ReadError, WriteError};
use embassy_nrf::{
    peripherals::QSPI,
    qspi::{Error as FlashError, Qspi},
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
const PAGE_SIZE: usize = 4096;

// Workaround for alignment requirements.
// Nicer API will probably come in the future.
#[repr(C, align(4))]
struct AlignedBuf([u8; PAGE_SIZE]);

async fn init_qspi(q: &mut Qspi<'_, QSPI>) {
    let mut id = [1; 3];
    unwrap!(q.custom_instruction(0x9F, &[], &mut id).await);
    info!("QSPI id: {}", id);

    // Read status register
    let mut status = [4; 1];
    unwrap!(q.custom_instruction(0x05, &[], &mut status).await);

    info!("QSPI status: {:?}", status[0]);

    if status[0] & 0x40 == 0 {
        status[0] |= 0x40;

        unwrap!(q.custom_instruction(0x01, &status, &mut []).await);

        info!("QSPI enabled quad in status");
    }
}

pub struct FlashCtrl<'a> {
    qspi: Qspi<'a, QSPI>,
    buf: AlignedBuf,
}

impl<'a> FlashCtrl<'a> {
    pub fn new(qspi: Qspi<'a, QSPI>) -> FlashCtrl<'a> {
        Self {
            qspi,
            buf: AlignedBuf([0; PAGE_SIZE]),
        }
    }
}

impl<'a> ekv::flash::Flash for FlashCtrl<'a> {
    type Error = embassy_nrf::qspi::Error;

    fn page_count(&self) -> usize {
        ekv::config::MAX_PAGE_COUNT
    }

    async fn erase(&mut self, page_id: ekv::flash::PageID) -> Result<(), Self::Error> {
        self.qspi.erase((page_id.index() * PAGE_SIZE) as u32).await
    }

    async fn read(
        &mut self,
        page_id: ekv::flash::PageID,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), Self::Error> {
        let address = page_id.index() * PAGE_SIZE + offset;
        self.qspi
            .read(address as u32, &mut self.buf.0[..data.len()])
            .await?;
        data.copy_from_slice(&self.buf.0[..data.len()]);
        Ok(())
    }

    async fn write(
        &mut self,
        page_id: ekv::flash::PageID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        let address = page_id.index() * PAGE_SIZE + offset;

        self.buf.0[..data.len()].copy_from_slice(data);
        self.qspi
            .write(address as u32, &self.buf.0[..data.len()])
            .await
    }
}

pub async fn init_db<'a>(
    mut q: Qspi<'a, QSPI>,
    seed: u32,
) -> Database<FlashCtrl<'a>, CriticalSectionRawMutex> {
    init_qspi(&mut q).await;
    let flash = FlashCtrl::new(q);

    let mut config = ekv::Config::default();

    let db: Database<FlashCtrl<'_>, CriticalSectionRawMutex> = ekv::Database::new(flash, config);

    if db.mount().await.is_err() {
        info!("Formatting DB");
        db.format().await.expect("Failed for format DB");
    }

    db
}
#[derive(Debug)]
pub enum DBReadError {
    IO(ReadError<FlashError>),
    Deserialize(postcard::Error),
}
impl From<postcard::Error> for DBReadError {
    fn from(value: postcard::Error) -> Self {
        DBReadError::Deserialize(value)
    }
}
impl From<ReadError<FlashError>> for DBReadError {
    fn from(value: ReadError<FlashError>) -> Self {
        DBReadError::IO(value)
    }
}
#[derive(Debug)]
pub enum DBWriteError {
    WriteError(WriteError<FlashError>),
    CommitError(CommitError<FlashError>),
    SerializeError(postcard::Error),
}

impl From<WriteError<FlashError>> for DBWriteError {
    fn from(value: WriteError<FlashError>) -> Self {
        DBWriteError::WriteError(value)
    }
}

impl From<CommitError<FlashError>> for DBWriteError {
    fn from(value: CommitError<FlashError>) -> Self {
        DBWriteError::CommitError(value)
    }
}
impl From<postcard::Error> for DBWriteError {
    fn from(value: postcard::Error) -> Self {
        Self::SerializeError(value)
    }
}

pub trait SerdeDB {
    type ReadError;
    type WriteError;
    async fn read<T: DeserializeOwned>(&self, key: impl AsRef<[u8]>) -> Result<T, Self::ReadError>;

    async fn write<T: Serialize>(
        &self,
        key: impl AsRef<[u8]>,
        val: &T,
    ) -> Result<(), Self::WriteError>;

    async fn read_key<T: DeserializeOwned + DBKey>(&self) -> Result<T, Self::ReadError> {
        self.read(T::KEY).await
    }

    async fn write_key<T: Serialize + DBKey>(&self, val: &T) -> Result<(), Self::WriteError> {
        self.write(T::KEY, val).await
    }
}

impl SerdeDB for Database<FlashCtrl<'_>, CriticalSectionRawMutex> {
    type ReadError = DBReadError;
    type WriteError = DBWriteError;

    async fn read<T: DeserializeOwned>(&self, key: impl AsRef<[u8]>) -> Result<T, Self::ReadError> {
        let mut rtx = self.read_transaction().await;
        let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];
        let r_len = rtx.read(key.as_ref(), &mut buf).await?;
        let data = &buf[..r_len];

        let data = postcard::from_bytes::<T>(data)?;
        Ok(data)
    }

    async fn write<T: Serialize>(
        &self,
        key: impl AsRef<[u8]>,
        val: &T,
    ) -> Result<(), Self::WriteError> {
        let mut wtx = self.write_transaction().await;
        let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];
        let buf = postcard::to_slice(val, &mut buf)?;

        wtx.write(key.as_ref(), buf).await?;

        wtx.commit().await?;

        Ok(())
    }
}

pub trait DBKey {
    const KEY: &'static [u8];
}
