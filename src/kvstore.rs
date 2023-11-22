use defmt::{info, unwrap, Format};
use ekv::config::PAGE_SIZE;
use ekv::WriteTransaction;
use ekv::{CommitError, Database, ReadError, WriteError};
use embassy_nrf::{
    peripherals::QSPI,
    qspi::{Error as FlashError, Qspi},
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use serde::{de::DeserializeOwned, Serialize};
use static_cell::StaticCell;

//https://www.mxic.com.tw/Lists/Datasheet/Attachments/8868/MX25R6435F,%20Wide%20Range,%2064Mb,%20v1.6.pdf
async fn init_qspi(q: &mut Qspi<'_, QSPI>) {
    let mut id = [1; 3];
    unwrap!(q.custom_instruction(0x9F, &[], &mut id).await);
    info!("id: {}", id);

    // Read status register
    let mut status = [4; 1];
    unwrap!(q.custom_instruction(0x05, &[], &mut status).await);

    info!("status: {:?}", status[0]);

    if status[0] & 0x40 == 0 {
        status[0] |= 0x40;

        unwrap!(q.custom_instruction(0x01, &status, &mut []).await);

        info!("enabled quad in status");
    }
    info!("QSPI Initalized")
}
// Workaround for alignment requirements.
#[repr(C, align(4))]
struct AlignedBuf([u8; PAGE_SIZE]);

static mut BUF: AlignedBuf = AlignedBuf([0; 4096]);
pub struct FlashCtrl {
    qspi: Qspi<'static, QSPI>,
}

impl FlashCtrl {
    pub fn new(qspi: Qspi<'static, QSPI>) -> FlashCtrl {
        Self { qspi }
    }
}

impl ekv::flash::Flash for FlashCtrl {
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
        unsafe {
            self.qspi
                .read(address as u32, &mut BUF.0[..data.len()])
                .await
                .unwrap();
            data.copy_from_slice(&BUF.0[..data.len()])
        }
        Ok(())
    }

    async fn write(
        &mut self,
        page_id: ekv::flash::PageID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        let address = page_id.index() * PAGE_SIZE + offset;

        unsafe {
            BUF.0[..data.len()].copy_from_slice(data);
            self.qspi
                .write(address as u32, &BUF.0[..data.len()])
                .await
                .unwrap();
        }
        Ok(())
    }
}

#[derive(Debug, Format)]
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
#[derive(Debug, Format)]
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
        wtx: &mut WriteTransaction<'_, FlashCtrl, NoopRawMutex>,
    ) -> Result<(), Self::WriteError>;
}

impl SerdeDB for Database<FlashCtrl, NoopRawMutex> {
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
        wtx: &mut WriteTransaction<'_, FlashCtrl, NoopRawMutex>,
    ) -> Result<(), Self::WriteError> {
        let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];
        let buf = postcard::to_slice(val, &mut buf)?;
        wtx.write(key.as_ref(), buf).await?;

        Ok(())
    }
}

pub trait DBKey {
    fn key(&self) -> &[u8];
}

pub type KVStore = Database<FlashCtrl, NoopRawMutex>;

static KVSTORE: StaticCell<KVStore> = StaticCell::new();

pub async fn init_kvstore(mut q: Qspi<'static, QSPI>) -> &'static KVStore {
    init_qspi(&mut q).await;
    let flash = FlashCtrl::new(q);

    let config = ekv::Config::default();

    let db: Database<FlashCtrl, NoopRawMutex> = ekv::Database::new(flash, config);
    db.format().await.expect("formatting failed");
    if db.mount().await.is_err() {
        info!("Formatting DB");
        db.format().await.expect("Failed for format DB");
    }

    let db = KVSTORE.init(db);
    info!("Initalized KV store");

    db
}
