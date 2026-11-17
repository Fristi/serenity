//! ES8311 speaker DAC on the shared I2C bus.
//!
//! The driver itself is `es8311-async`; this module only chooses the clocking
//! and the volume this board uses, and shuts down the microphone path the
//! driver always starts. The codec is an I2S slave clocked from the MCLK pin at
//! 4.096 MHz = 16000 * 256, which [`Config`] resolves with the pre-divider 1 /
//! BCLK divider 4 pair the Waveshare reference uses.
//!
//! This board has no speaker-amp enable GPIO, so the codec is the only thing
//! that can silence the speaker. [`init`] leaves the DAC muted, and [`unmute`]
//! is called once the I2S bus is streaming zeros, which keeps the amp from
//! amplifying a floating data line at boot.

use anyhow::anyhow;
use embedded_hal_async::i2c::I2c;
use es8311_async::{Config, Es8311};

use crate::audio::SAMPLE_RATE_HZ;

/// Codec address on this board (CE held low).
const ADDR: u8 = es8311_async::DEFAULT_ADDRESS;

/// The Waveshare reference writes `0xD9` to the volume register, which the
/// ES8311 encodes as +13 dB (`0xBF` is 0 dB, two steps per dB).
const VOLUME_DB: i8 = 13;

/// Analog PGA and ADC modulator enable (the driver's private `SYSTEM_0E`).
const REG_SYSTEM_0E: u8 = 0x0E;
/// Analog input select and PGA gain (the driver's private `SYSTEM_14`).
const REG_SYSTEM_14: u8 = 0x14;
/// Everything in `SYSTEM_0E` off: the mic PGA and ADC modulator.
const SYS0E_ADC_OFF: u8 = 0xFF;
/// No analog input selected, PGA at 0 dB.
const SYS14_NO_INPUT: u8 = 0x00;

pub type Codec<I> = Es8311<I>;

/// Configure the DAC for 16 kHz playback and leave it muted.
///
/// MCLK, BCLK and WS must already be running: call this after the I2S
/// peripheral is streaming.
pub async fn init<I>(i2c: I) -> anyhow::Result<Codec<I>>
where
    I: I2c,
    I::Error: core::fmt::Debug,
{
    let mut codec = Es8311::new(i2c, ADDR);
    codec
        .init(Config {
            sample_rate: SAMPLE_RATE_HZ,
            mclk_hz: 256 * SAMPLE_RATE_HZ,
        })
        .await
        .map_err(|e| anyhow!("ES8311 init failed: {e:?}"))?;

    // The driver always starts the ADC path. This board has no microphone, so
    // power it back down rather than leave the PGA and modulator running.
    let mut bus = codec.release();
    bus.write(ADDR, &[REG_SYSTEM_0E, SYS0E_ADC_OFF])
        .await
        .map_err(|e| anyhow!("ES8311 mic power-down failed: {e:?}"))?;
    bus.write(ADDR, &[REG_SYSTEM_14, SYS14_NO_INPUT])
        .await
        .map_err(|e| anyhow!("ES8311 mic power-down failed: {e:?}"))?;

    Ok(Es8311::new(bus, ADDR))
}

/// Raise the volume to [`VOLUME_DB`] and unmute the DAC.
pub async fn unmute<I>(codec: &mut Codec<I>) -> anyhow::Result<()>
where
    I: I2c,
    I::Error: core::fmt::Debug,
{
    codec
        .set_dac_volume_db(VOLUME_DB)
        .await
        .map_err(|e| anyhow!("ES8311 volume failed: {e:?}"))?;
    codec
        .set_dac_mute(false)
        .await
        .map_err(|e| anyhow!("ES8311 unmute failed: {e:?}"))
}
