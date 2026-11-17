//! ES8311 speaker DAC on the shared I2C bus.
//!
//! Only the playback path is configured: 16- or 32-bit I2S at 16 kHz with MCLK
//! supplied on the pin (4.096 MHz = 16000 * 256). The register sequence mirrors
//! Waveshare's `es8311_init()` for this board, which the esp32c6-watch project
//! has also verified with esp-hal on the sibling 2.06" panel.
//!
//! This board has no speaker-amp enable GPIO, so the codec is the only thing
//! that can silence the speaker. [`Es8311::init`] therefore leaves the master
//! volume at zero: the caller raises it once the I2S bus is streaming zeros,
//! which keeps the amp from amplifying a floating data line at boot.

use embedded_hal_async::i2c::I2c;

const ADDR: u8 = 0x18;

/// Master volume register value: 0x00 is silent, 0xFF is max. 0xD9 is ~85%,
/// the value the Waveshare reference uses.
pub const VOLUME_85: u8 = 0xD9;
/// Silent setting for register 0x32.
pub const MUTE: u8 = 0x00;

pub struct Es8311<I> {
    i2c: I,
}

impl<I: I2c> Es8311<I> {
    pub fn new(i2c: I) -> Self {
        Self { i2c }
    }

    async fn write(&mut self, reg: u8, val: u8) -> Result<(), I::Error> {
        self.i2c.write(ADDR, &[reg, val]).await
    }

    async fn read(&mut self, reg: u8) -> Result<u8, I::Error> {
        let mut buf = [0u8];
        self.i2c.write_read(ADDR, &[reg], &mut buf).await?;
        Ok(buf[0])
    }

    /// Reset and configure the DAC for 16 kHz I2S playback, muted.
    ///
    /// The DAC and headphone driver are left powered up; only the volume
    /// register is zero, so [`set_volume`](Self::set_volume) brings the sound
    /// back without a power-up transient.
    pub async fn init(&mut self) -> Result<(), I::Error> {
        // Reset, then the mandatory power-on command after it.
        self.write(0x00, 0x1F).await?;
        self.write(0x00, 0x00).await?;
        self.write(0x00, 0x80).await?;

        // MCLK from the pin; coefficients for 4.096 MHz / 16 kHz.
        self.write(0x01, 0x3F).await?;

        // pre_div = 2, pre_multi = 1x
        let reg02 = (self.read(0x02).await.unwrap_or(0) & 0x07) | (2 - 1) << 5;
        self.write(0x02, reg02).await?;

        self.write(0x03, 0x10).await?; // fs_mode = 0, adc_osr = 0x10
        self.write(0x04, 0x10).await?; // dac_osr
        // adc_div = dac_div = 1, written as the vendor's (div - 1) fields.
        let adc_div = 1u8;
        let dac_div = 1u8;
        self.write(0x05, ((adc_div - 1) << 4) | (dac_div - 1)).await?;

        // bclk_div = 4
        let reg06 = (self.read(0x06).await.unwrap_or(0) & 0xE0) | (4 - 1);
        self.write(0x06, reg06).await?;

        // lrck = 0x00FF
        let reg07 = self.read(0x07).await.unwrap_or(0) & 0xC0;
        self.write(0x07, reg07).await?;
        self.write(0x08, 0xFF).await?;

        // Serial data port: 16-bit I2S for both DAC and ADC.
        self.write(0x09, 0x0C).await?;
        self.write(0x0A, 0x0C).await?;

        // Analog power-up, in the vendor's order.
        self.write(0x0D, 0x01).await?;
        self.write(0x0E, 0x02).await?;
        self.write(0x12, 0x00).await?; // DAC on
        self.write(0x13, 0x10).await?; // HP drive on
        self.write(0x1C, 0x6A).await?;
        self.write(0x37, 0x08).await?;

        // Silent until the caller knows the I2S line is driven.
        self.write(0x32, MUTE).await
    }

    /// Write the master volume register directly (see [`VOLUME_85`]).
    pub async fn set_volume(&mut self, reg: u8) -> Result<(), I::Error> {
        self.write(0x32, reg).await
    }
}
