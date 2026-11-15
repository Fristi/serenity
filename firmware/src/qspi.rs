//! QSPI Device Adapter for esp-hal
//!
//! `esp-hal`'s slice-based DMA API takes `&[u8]` / `&mut [u8]` directly and
//! copies through an internal DMA buffer when the caller's slice is not
//! directly usable by the DMA engine (e.g. flash-resident init commands).
//! That removes the manual DRAM bounce buffer this adapter used to carry: see
//! [`SpiDma::half_duplex_write_async`] for the copy heuristic.

use display_driver_qspi::{LineMode, PhaseConfig, QspiDevice, QspiTransaction};
use esp_hal::spi::master::{Address, Command, DataMode, SpiDma};

/// DMA copy buffers for the QSPI bus.
///
/// The panel init sequence and pixel data are both larger than
/// [`CHUNK_SIZE`](esp_hal::dma::CHUNK_SIZE), so the copy buffers are sized to
/// hold one full-width transfer.
const DMA_BUF_SIZE: usize = 32736;

/// Wraps an esp-hal `SpiDma<Async>` to implement the [`QspiDevice`] trait
/// required by `QspiDisplayBus`.
pub struct EspHalQspiDevice<'d> {
    pub spi: SpiDma<'d, esp_hal::Async>,
}

impl<'d> QspiDevice for EspHalQspiDevice<'d> {
    type Error = esp_hal::spi::Error;

    async fn write(
        &mut self,
        transaction: &QspiTransaction,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        let cmd = to_esp_command(&transaction.instruction);
        let addr = to_esp_address(&transaction.address);
        let data_mode = to_esp_data_mode(transaction.data_mode);

        self.spi
            .half_duplex_write_async(
                data_mode,
                cmd,
                addr,
                transaction.dummy_cycles,
                data,
            )
            .await
    }

    async fn read(
        &mut self,
        transaction: &QspiTransaction,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        let cmd = to_esp_command(&transaction.instruction);
        let addr = to_esp_address(&transaction.address);
        let data_mode = to_esp_data_mode(transaction.data_mode);

        self.spi
            .half_duplex_read_async(
                data_mode,
                cmd,
                addr,
                transaction.dummy_cycles,
                buffer,
            )
            .await
    }
}

/// Create the DMA copy buffers the slice-based API copies through.
pub fn dma_buffers() -> anyhow::Result<(esp_hal::dma::DmaRxBuf, esp_hal::dma::DmaTxBuf)> {
    let rx = esp_hal::dma_rx_buffer!(DMA_BUF_SIZE)
        .map_err(|e| anyhow::anyhow!("DMA RX buffer alloc failed: {e}"))?;
    let tx = esp_hal::dma_tx_buffer!(DMA_BUF_SIZE)
        .map_err(|e| anyhow::anyhow!("DMA TX buffer alloc failed: {e}"))?;
    Ok((rx, tx))
}

// ---------------------------------------------------------------------------
// Type-conversion helpers (display-driver types → esp-hal types)
// ---------------------------------------------------------------------------

/// Map [`LineMode`] to esp-hal [`DataMode`].
fn to_esp_data_mode(mode: LineMode) -> DataMode {
    match mode {
        LineMode::None | LineMode::Single => DataMode::Single,
        LineMode::Dual => DataMode::Dual,
        LineMode::Quad => DataMode::Quad,
    }
}

/// Map an optional instruction [`PhaseConfig`] to esp-hal [`Command`].
fn to_esp_command(phase: &Option<PhaseConfig>) -> Command {
    match phase {
        None => Command::None,
        Some(cfg) => {
            let mode = to_esp_data_mode(cfg.mode);
            match cfg.bytes_len {
                2 => Command::_16Bit(cfg.value as u16, mode),
                _ => Command::_8Bit(cfg.value as u16, mode),
            }
        }
    }
}

/// Map an optional address [`PhaseConfig`] to esp-hal [`Address`].
fn to_esp_address(phase: &Option<PhaseConfig>) -> Address {
    match phase {
        None => Address::None,
        Some(cfg) => {
            let mode = to_esp_data_mode(cfg.mode);
            match cfg.bytes_len {
                1 => Address::_8Bit(cfg.value, mode),
                2 => Address::_16Bit(cfg.value, mode),
                4 => Address::_32Bit(cfg.value, mode),
                _ => Address::_24Bit(cfg.value, mode),
            }
        }
    }
}
