//! Input gain. It is a standard USB Audio control, not part of the HID protocol.

use alsa::mixer::{Mixer, Selem, SelemChannelId, SelemId};
use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Gain {
    pub value: i64,
    pub min: i64,
    pub max: i64,
    pub db_min: f32,
    pub db_max: f32,
}

fn err(e: alsa::Error) -> Error {
    Error::Mixer(e.to_string())
}

fn with_mic<T>(card: u32, f: impl FnOnce(&Selem) -> Result<T>) -> Result<T> {
    let mixer = Mixer::new(&format!("hw:{card}"), false).map_err(err)?;
    let selem = mixer
        .find_selem(&SelemId::new("Mic", 0))
        .filter(Selem::has_capture_volume)
        .ok_or_else(|| Error::Mixer("the card has no 'Mic' capture volume".into()))?;
    f(&selem)
}

fn read(selem: &Selem) -> Result<Gain> {
    let (min, max) = selem.get_capture_volume_range();
    let (db_min, db_max) = selem.get_capture_db_range();
    let value = selem.get_capture_volume(SelemChannelId::mono()).map_err(err)?;
    Ok(Gain { value, min, max, db_min: db_min.to_db(), db_max: db_max.to_db() })
}

pub fn gain(card: u32) -> Result<Gain> {
    with_mic(card, read)
}

pub fn set_gain(card: u32, value: i64) -> Result<Gain> {
    with_mic(card, |selem| {
        let (min, max) = selem.get_capture_volume_range();
        selem.set_capture_volume_all(value.clamp(min, max)).map_err(err)?;
        read(selem)
    })
}
