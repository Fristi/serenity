//! Panel bring-up for the Waveshare ESP32-C6-Touch-AMOLED-2.16.
//!
//! Owns the QSPI wiring, the CO5300 panel spec and the DMA copy buffers the
//! slice-based spi driver transfers through.

use core::cell::RefCell;

use anyhow::anyhow;
use axp2101_embedded::{AsyncAxp2101, Error as AxpError};
use display_driver::{
    panel::reset::NoResetPin, ColorFormat, DisplayDriver, LCDResetOption,
};
use display_driver_co5300::{
    spec::{Co5300Spec, PanelSpec},
    Co5300,
};
use display_driver_qspi::{QspiConfig, QspiDisplayBus};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c as AsyncI2c;
use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    i2c::master::I2c,
    peripherals::{DMA_CH0, GPIO0, GPIO1, GPIO2, GPIO3, GPIO4, GPIO15, SPI2},
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
    Async,
};

use crate::i2c::AsyncRefCellDevice;
use crate::qspi::{dma_buffers, EspHalQspiDevice};

/// Value read back from `IC_TYPE` (0x03) on an AXP2101.
///
/// `axp2101-embedded`'s `init()` checks for 0x47, which this chip does not
/// report, so the rails are configured explicitly instead of calling it.
const AXP2101_CHIP_ID: u8 = 0x4A;

/// Waveshare ESP32-C6-Touch-AMOLED-2.16 panel: 480x480 CO5300 AMOLED.
///
/// `GenericCo5300` in the driver crate is a 240x240 placeholder with a
/// different init page, so it cannot be used for this panel.
pub struct WaveshareAmoled480x480;

impl PanelSpec for WaveshareAmoled480x480 {
    const PHYSICAL_WIDTH: u16 = 480;
    const PHYSICAL_HEIGHT: u16 = 480;
    const PHYSICAL_X_OFFSET: u16 = 0;
    const PHYSICAL_Y_OFFSET: u16 = 0;
}

impl Co5300Spec for WaveshareAmoled480x480 {
    const INIT_PAGE_PARAM: u8 = 0x00;
    const IGNORE_ID_CHECK: bool = true;
}

/// QSPI pins on the 2.16 board: CS=15, PCLK=0, D0..D3=1..4.
pub struct QspiPins {
    pub cs: GPIO15<'static>,
    pub sck: GPIO0<'static>,
    pub sio0: GPIO1<'static>,
    pub sio1: GPIO2<'static>,
    pub sio2: GPIO3<'static>,
    pub sio3: GPIO4<'static>,
}

/// Fully initialized display for the on-board panel.
pub type BoardDisplay = DisplayDriver<
    QspiDisplayBus<EspHalQspiDevice<'static>>,
    Co5300<WaveshareAmoled480x480, NoResetPin, QspiDisplayBus<EspHalQspiDevice<'static>>>,
>;

/// Configure the PMIC rails the panel and peripherals run from.
async fn init_pmic<I>(pmic: &mut AsyncAxp2101<I>) -> Result<(), AxpError<I::Error>>
where
    I: AsyncI2c,
{
    let chip_id = pmic.get_chip_id().await?;
    if chip_id != AXP2101_CHIP_ID {
        defmt::warn!("Unexpected AXP2101 chip ID: {=u8:#04x}", chip_id);
    }

    // DC1 = 3300mV (main 3.3V rail).
    pmic.enable_dc1().await?;
    pmic.set_dc1_voltage(3300).await?;

    // ALDO1-3 = 3300mV; ALDO3 is the panel rail.
    pmic.set_aldo1_voltage(3300).await?;
    pmic.set_aldo2_voltage(3300).await?;
    pmic.set_aldo3_voltage(3300).await?;
    pmic.enable_aldo1().await?;
    pmic.enable_aldo2().await?;
    pmic.enable_aldo3().await?;

    Ok(())
}

/// Power-cycle the panel rail (ALDO3).
///
/// The 2.16 board has no LCD reset pin: Waveshare resets the panel by
/// power-cycling the ALDO3 rail. Without this the panel ignores init.
pub async fn power_cycle_panel(i2c: &RefCell<I2c<'static, Async>>) -> anyhow::Result<()> {
    let mut pmic = AsyncAxp2101::new(AsyncRefCellDevice::new(i2c));
    init_pmic(&mut pmic)
        .await
        .map_err(|e| anyhow!("AXP2101 init failed: {e:?}"))?;

    defmt::info!("Power-cycling panel rail (ALDO3)");
    pmic.enable_aldo3()
        .await
        .map_err(|e| anyhow!("ALDO3 on failed: {e:?}"))?;
    Timer::after(Duration::from_millis(100)).await;
    pmic.disable_aldo3()
        .await
        .map_err(|e| anyhow!("ALDO3 off failed: {e:?}"))?;
    Timer::after(Duration::from_millis(100)).await;
    pmic.enable_aldo3()
        .await
        .map_err(|e| anyhow!("ALDO3 on failed: {e:?}"))?;
    Timer::after(Duration::from_millis(100)).await;

    Ok(())
}

/// Bring up the QSPI bus and the CO5300 panel, leaving the backlight on full.
pub async fn init(
    spi: SPI2<'static>,
    pins: QspiPins,
    dma: DMA_CH0<'static>,
    delay: &mut impl DelayNs,
) -> anyhow::Result<BoardDisplay> {
    let (dma_rx_buf, dma_tx_buf) = dma_buffers()?;

    let cs = Output::new(pins.cs, Level::High, OutputConfig::default());
    let spi = Spi::new(
        spi,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(80))
            .with_mode(esp_hal::spi::Mode::_0),
    )
    .map_err(|e| anyhow!("SPI init failed: {e}"))?
    .with_sck(pins.sck)
    .with_sio0(pins.sio0)
    .with_sio1(pins.sio1)
    .with_sio2(pins.sio2)
    .with_sio3(pins.sio3)
    .with_cs(cs)
    .with_dma(dma)
    .with_buffers(dma_rx_buf, dma_tx_buf)
    .into_async();

    let device = EspHalQspiDevice { spi };

    // Default QspiConfig: cmd_data_mode=Single, ram_data_mode=Quad (1-1-4)
    let bus = QspiDisplayBus::new(device, QspiConfig::default());
    let panel = Co5300::<WaveshareAmoled480x480, NoResetPin, _>::new(LCDResetOption::none());

    let mut disp = DisplayDriver::builder(bus, panel)
        .with_color_format(ColorFormat::RGB565)
        .init(delay)
        .await
        .map_err(|e| anyhow!("Panel init failed: {e:?}"))?;

    disp.set_brightness(0xff)
        .await
        .map_err(|e| anyhow!("Set brightness failed: {e:?}"))?;
    defmt::info!("Brightness set to 255.");
    Ok(disp)
}

/// Delay provider used for panel initialization.
pub fn delay() -> Delay {
    Delay
}
