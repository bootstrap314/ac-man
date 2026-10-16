//! Raw Midea AC frames; wire formats cross-checked against msmart-ng 2026.9.0.
use crate::{Result, SettingsChange, problem};
use std::collections::BTreeMap;

pub fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x8c
            } else {
                crc >> 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> Vec<u8> {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/protocol.json")).unwrap();
        hex::decode(data[name].as_str().unwrap()).unwrap()
    }
    #[test]
    fn commands_match_independent_python_reference() {
        assert_eq!(state_query(1), fixture("state_query"));
        assert_eq!(capability_query(false, 1), fixture("capability_query"));
        assert_eq!(capability_query(true, 1), fixture("additional_query"));
        let mut state = State::decode(&fixture("state_response")).unwrap();
        assert!(state.power);
        assert_eq!(state.temperature, 21.0);
        assert_eq!(state.fan, 102);
        let caps = Capabilities::decode(&fixture("capabilities_response")).unwrap();
        assert!(caps.custom_fan);
        assert!(caps.boost);
        assert!(caps.modes.contains(&2));
        state
            .apply(
                &SettingsChange {
                    fan: Some(37),
                    boost: Some(true),
                    ..Default::default()
                },
                &caps,
            )
            .unwrap();
        assert_eq!(state.encode(1), fixture("set_from_state"));
    }
    #[test]
    fn eco_capabilities_commands_and_readback() {
        // msmart-ng PRESET_ECO (0x0212) supports values 1 and 2.
        for value in [0, 1, 2, 3, 255] {
            let caps =
                Capabilities::decode(&frame(3, vec![0xb5, 1, 0x12, 2, 1, value], 1)).unwrap();
            assert_eq!(caps.eco, matches!(value, 1 | 2));
            for enabled in [true, false] {
                let change = SettingsChange {
                    eco: Some(enabled),
                    ..Default::default()
                };
                let mut state = State::decode(&fixture("state_response")).unwrap();
                if !caps.eco {
                    assert!(state.apply(&change, &caps).is_err());
                    continue;
                }
                state.apply(&change, &caps).unwrap();
                assert_eq!(payload(&state.encode(1)).unwrap()[9] & 0x80 != 0, enabled);
                // Status uses bit 0x10, unlike the command's 0x80.
                let mut response = payload(&fixture("state_response")).unwrap().to_vec();
                response[9] = (response[9] & !0x10) | if enabled { 0x10 } else { 0 };
                let actual = State::decode(&frame(3, response, 1)).unwrap();
                assert!(actual.matches(&change));
                assert!(!actual.matches(&SettingsChange {
                    eco: Some(!enabled),
                    ..Default::default()
                }));
            }
        }
        let caps = Capabilities::decode(&fixture("capabilities_response")).unwrap();
        assert!(!caps.eco);
        assert!(
            caps.validate(&SettingsChange {
                eco: Some(true),
                ..Default::default()
            })
            .is_err()
        );
    }
    #[test]
    fn additional_capabilities_remain_pending_until_merged() {
        let mut caps = Capabilities::decode(&frame(3, vec![0xb5, 1, 0x10, 2, 1, 1, 1], 1)).unwrap();
        assert!(caps.additional);
        assert!(caps.custom_fan);
        assert!(!caps.vertical_angle);
        // No reply or a malformed reply must not change the cached first page.
        assert!(Capabilities::decode(&frame(3, vec![0xb5, 1], 1)).is_err());
        assert!(caps.additional);
        let additional =
            Capabilities::decode(&frame(3, vec![0xb5, 2, 9, 0, 1, 1, 0x15, 2, 1, 1, 0], 2))
                .unwrap();
        caps.merge(additional);
        assert!(!caps.additional);
        assert!(caps.custom_fan);
        assert!(caps.vertical_angle);
        assert_eq!(caps.swings, vec![0, 3, 12, 15]);
    }
    #[test]
    fn vent_frames_match_msmart_ng_2026_9_0() {
        let caps =
            Capabilities::decode(&frame(3, vec![0xb5, 2, 9, 0, 1, 1, 10, 0, 1, 1], 1)).unwrap();
        assert_eq!(
            hex::encode(angle_query(&caps, 1).unwrap()),
            "aa12ac00000000000003b10209000a00011761"
        );
        let change = SettingsChange {
            vertical_angle: Some(25),
            horizontal_angle: Some(75),
            ..Default::default()
        };
        caps.validate(&change).unwrap();
        assert_eq!(
            hex::encode(angle_command(&change, 1).unwrap()),
            "aa16ac00000000000002b002090001190a00014b012de3"
        );
        let unsupported =
            Capabilities::decode(&frame(3, vec![0xb5, 2, 9, 0, 1, 0, 10, 0, 1, 2], 1)).unwrap();
        assert!(angle_query(&unsupported, 1).is_none());
        assert!(unsupported.validate(&change).is_err());
        assert!(
            caps.validate(&SettingsChange {
                vertical_angle: Some(45),
                ..Default::default()
            })
            .is_err()
        );
    }
    #[test]
    fn vent_responses_reject_bad_data_and_do_not_confirm_unknown_positions() {
        let bytes = frame(3, vec![0xb1, 2, 9, 0, 0, 1, 25, 10, 0, 0, 1, 75], 1);
        assert_eq!(decode_angles(&bytes).unwrap(), (Some(25), Some(75)));
        assert_eq!(
            decode_angles(&frame(
                3,
                vec![0xb1, 2, 9, 0, 0x10, 1, 25, 10, 0, 0, 1, 42],
                1
            ))
            .unwrap(),
            (None, None)
        );
        assert!(decode_angles(&frame(3, vec![0xb1, 1, 9, 0, 0, 20, 25], 1)).is_err());
        for end in 0..bytes.len() {
            assert!(decode_angles(&bytes[..end]).is_err());
        }
        // A valid outer checksum cannot make a sequence byte a vent value.
        for sequence in [1, 25, 50, 75, 100] {
            assert!(decode_angles(&frame(3, vec![0xb1, 1, 9, 0, 0, 1], sequence)).is_err());
        }
        let mut state = State::decode(&fixture("state_response")).unwrap();
        let change = SettingsChange {
            vertical_angle: Some(25),
            ..Default::default()
        };
        assert!(!state.matches(&change));
        state.vertical_angle = Some(25);
        assert!(state.matches(&change));
    }
    #[test]
    fn corrupt_and_truncated_frames_are_rejected() {
        let mut bytes = fixture("state_response");
        for end in 0..bytes.len() {
            assert!(State::decode(&bytes[..end]).is_err());
        }
        bytes[13] ^= 1;
        assert!(State::decode(&bytes).is_err());
    }
    #[test]
    fn percentages_temperature_and_capabilities_are_validated() {
        let caps = Capabilities::decode(&fixture("capabilities_response")).unwrap();
        for value in [1, 37, 96, 100, 102] {
            assert!(
                caps.validate(&SettingsChange {
                    fan: Some(value),
                    ..Default::default()
                })
                .is_ok()
            );
        }
        for value in [0, -1, 101, 103] {
            assert!(
                caps.validate(&SettingsChange {
                    fan: Some(value),
                    ..Default::default()
                })
                .is_err()
            );
        }
        for value in [f64::NAN, f64::INFINITY, 15.0, 31.0, 23.1] {
            assert!(
                caps.validate(&SettingsChange {
                    temperature: Some(value),
                    ..Default::default()
                })
                .is_err()
            );
        }
        assert!(caps.validate(&SettingsChange::default()).is_err());
        assert!(
            caps.validate(&SettingsChange {
                mode: Some(4),
                ..Default::default()
            })
            .is_err()
        );
    }
}
fn checksum(bytes: &[u8]) -> u8 {
    0u8.wrapping_sub(bytes.iter().fold(0u8, |s, b| s.wrapping_add(*b)))
}
pub fn frame(kind: u8, mut payload: Vec<u8>, sequence: u8) -> Vec<u8> {
    payload.push(sequence);
    payload.push(crc8(&payload));
    let mut frame = vec![
        0xaa,
        (payload.len() + 10) as u8,
        0xac,
        0,
        0,
        0,
        0,
        0,
        0,
        kind,
    ];
    frame.extend(payload);
    frame.push(checksum(&frame[1..]));
    frame
}
pub fn state_query(sequence: u8) -> Vec<u8> {
    frame(
        3,
        vec![
            0x41, 0x81, 0, 0xff, 3, 0xff, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3,
        ],
        sequence,
    )
}
pub fn capability_query(extra: bool, sequence: u8) -> Vec<u8> {
    frame(
        3,
        if extra {
            vec![0xb5, 1, 1, 1]
        } else {
            vec![0xb5, 1, 0]
        },
        sequence,
    )
}
pub fn payload(frame: &[u8]) -> Result<&[u8]> {
    if frame.len() < 14
        || frame[0] != 0xaa
        || frame[2] != 0xac
        || frame[1] as usize + 1 != frame.len()
    {
        return Err(problem(
            "The AC returned an incomplete or unrecognized frame.",
        ));
    }
    if checksum(&frame[1..frame.len() - 1]) != frame[frame.len() - 1] {
        return Err(problem("The AC response failed its frame checksum."));
    }
    let body = &frame[10..frame.len() - 1];
    let last = body.len() - 1;
    if crc8(&body[..last]) != body[last] && checksum(&body[..last]) != body[last] {
        return Err(problem("The AC response failed its payload checksum."));
    }
    Ok(&body[..last])
}

#[derive(Clone, Debug)]
pub struct State {
    pub power: bool,
    pub temperature: f64,
    pub mode: i32,
    pub fan: i32,
    pub swing: i32,
    pub vertical_angle: Option<i32>,
    pub horizontal_angle: Option<i32>,
    pub boost: bool,
    pub indoor: Option<f64>,
    pub outdoor: Option<f64>,
    pub eco: bool,
    sleep: bool,
    fahrenheit: bool,
    purifier: bool,
    aux_heat: bool,
    independent_aux_heat: bool,
    follow_me: bool,
    freeze: bool,
    humidity: u8,
}
fn ambient(raw: u8, decimal: u8, fahrenheit: bool) -> Option<f64> {
    if raw == 255 {
        return None;
    }
    let base = (f64::from(raw) - 50.0) / 2.0;
    let decimal = f64::from(decimal) / 10.0;
    let sign = if base >= 0.0 { 1.0 } else { -1.0 };
    Some(if !fahrenheit && decimal > 0.0 {
        base.trunc() + sign * decimal
    } else if decimal >= 0.5 {
        base.trunc() + sign * 0.5
    } else {
        base
    })
}
impl State {
    pub fn decode(frame: &[u8]) -> Result<Self> {
        let p = payload(frame)?;
        if p.len() < 17 || p[0] != 0xc0 {
            return Err(problem("The AC did not return a current state."));
        }
        let half = if p[2] & 0x10 != 0 { 0.5 } else { 0.0 };
        let alternate = p[13] & 0x1f;
        let temperature = if alternate != 0 {
            f64::from(alternate) + 12.0
        } else {
            f64::from(p[2] & 15) + 16.0
        } + half;
        let fahrenheit = p[10] & 4 != 0;
        Ok(Self {
            power: p[1] & 1 != 0,
            temperature,
            mode: i32::from((p[2] >> 5) & 7),
            fan: i32::from(p[3] & 0x7f),
            swing: i32::from(p[7] & 15),
            vertical_angle: None,
            horizontal_angle: None,
            boost: p[8] & 0x20 != 0 || p[10] & 2 != 0,
            indoor: ambient(p[11], p[15] & 15, fahrenheit),
            outdoor: ambient(p[12], p[15] >> 4, fahrenheit),
            eco: p[9] & 0x10 != 0,
            sleep: p[10] & 1 != 0,
            fahrenheit,
            purifier: p[9] & 0x20 != 0,
            aux_heat: p[9] & 8 != 0,
            independent_aux_heat: p[8] & 0x40 != 0,
            follow_me: p[8] & 0x80 != 0,
            freeze: p.get(21).is_some_and(|v| v & 0x80 != 0),
            humidity: p.get(19).copied().unwrap_or(40) & 0x7f,
        })
    }
    pub fn encode(&self, sequence: u8) -> Vec<u8> {
        let whole = self.temperature.floor() as u8;
        let (t, alt) = if (16..=31).contains(&whole) {
            (whole - 16, 0)
        } else {
            (0, whole.saturating_sub(12) & 31)
        };
        let mut p = vec![0u8; 24];
        p[0] = 0x40;
        p[1] = 2 | u8::from(self.power);
        p[2] =
            t | if self.temperature.fract() != 0.0 {
                0x10
            } else {
                0
            } | ((self.mode as u8 & 7) << 5);
        p[3] = self.fan as u8;
        p[4] = 0x7f;
        p[5] = 0x7f;
        p[7] = 0x30 | (self.swing as u8 & 0x3f);
        p[8] = (if self.follow_me { 0x80 } else { 0 }) | if self.boost { 0x20 } else { 0 };
        p[9] = (if self.eco { 0x80 } else { 0 })
            | (if self.purifier { 0x20 } else { 0 })
            | if self.aux_heat { 8 } else { 0 };
        p[10] = u8::from(self.sleep)
            | (if self.boost { 2 } else { 0 })
            | if self.fahrenheit { 4 } else { 0 };
        p[18] = alt;
        p[19] = self.humidity;
        p[21] = if self.freeze { 0x80 } else { 0 };
        p[22] = if self.independent_aux_heat { 8 } else { 0 };
        frame(2, p, sequence)
    }
    pub fn apply(&mut self, change: &SettingsChange, caps: &Capabilities) -> Result<()> {
        caps.validate(change)?;
        if let Some(v) = change.power {
            self.power = v;
        }
        if let Some(v) = change.temperature {
            self.temperature = v;
        }
        if let Some(v) = change.mode {
            self.mode = v;
        }
        if let Some(v) = change.fan {
            self.fan = v;
        }
        if let Some(v) = change.swing {
            self.swing = v;
        }
        if let Some(v) = change.boost {
            self.boost = v;
        }
        if let Some(v) = change.eco {
            self.eco = v;
        }
        Ok(())
    }
    pub fn matches(&self, c: &SettingsChange) -> bool {
        c.power.is_none_or(|v| v == self.power)
            && c.temperature
                .is_none_or(|v| (v - self.temperature).abs() < 0.01)
            && c.mode.is_none_or(|v| v == self.mode)
            && c.fan.is_none_or(|v| v == self.fan)
            && c.swing.is_none_or(|v| v == self.swing)
            && c.vertical_angle
                .is_none_or(|v| Some(v) == self.vertical_angle)
            && c.horizontal_angle
                .is_none_or(|v| Some(v) == self.horizontal_angle)
            && c.boost.is_none_or(|v| v == self.boost)
            && c.eco.is_none_or(|v| v == self.eco)
    }
}

#[derive(Clone, Debug)]
pub struct Capabilities {
    raw: BTreeMap<u16, Vec<u8>>,
    pub additional: bool,
    pub modes: Vec<i32>,
    pub fans: Vec<i32>,
    pub swings: Vec<i32>,
    pub vertical_angle: bool,
    pub horizontal_angle: bool,
    pub custom_fan: bool,
    pub boost: bool,
    pub eco: bool,
    pub min_temp: f64,
    pub max_temp: f64,
}
impl Capabilities {
    pub fn decode(frame: &[u8]) -> Result<Self> {
        let p = payload(frame)?;
        if p.len() < 2 || p[0] != 0xb5 {
            return Err(problem("The AC did not return its supported settings."));
        }
        let mut raw = BTreeMap::new();
        let mut offset = 2;
        for _ in 0..p[1] {
            if offset + 3 > p.len() {
                return Err(problem("Incomplete AC capabilities."));
            }
            let id = u16::from_le_bytes([p[offset], p[offset + 1]]);
            let len = p[offset + 2] as usize;
            offset += 3;
            if offset + len > p.len() {
                return Err(problem("Incomplete AC capabilities."));
            }
            if len > 0 {
                raw.insert(id, p[offset..offset + len].to_vec());
            }
            offset += len;
        }
        let additional = p.len().saturating_sub(offset) > 1 && p[p.len() - 2] != 0;
        let mut c = Self {
            raw,
            additional,
            modes: vec![],
            fans: vec![],
            swings: vec![],
            vertical_angle: false,
            horizontal_angle: false,
            custom_fan: false,
            boost: false,
            eco: false,
            min_temp: 16.0,
            max_temp: 30.0,
        };
        c.update();
        Ok(c)
    }
    pub fn merge(&mut self, other: Self) {
        self.additional = false;
        self.raw.extend(other.raw);
        self.update();
    }
    fn value(&self, id: u16) -> Option<u8> {
        self.raw.get(&id).and_then(|v| v.first().copied())
    }
    fn update(&mut self) {
        self.vertical_angle = self.value(0x0009) == Some(1);
        self.horizontal_angle = self.value(0x000a) == Some(1);
        self.modes = vec![5];
        if let Some(v) = self.value(0x214) {
            if [0, 1, 5, 6, 9, 11, 13, 14, 15].contains(&v) {
                self.modes.push(3);
            }
            if [0, 1, 3, 4, 5, 6, 7, 8, 9, 11, 13, 14, 15].contains(&v) {
                self.modes.push(2);
            }
            if [1, 2, 4, 6, 7, 9, 10, 11, 12, 13].contains(&v) {
                self.modes.push(4);
            }
            if [0, 1, 2, 7, 8, 9, 13, 14].contains(&v) {
                self.modes.push(1);
            }
        }
        if self.value(0x7c) == Some(1) {
            self.modes.push(6);
        }
        self.swings = vec![0];
        if let Some(v) = self.value(0x215) {
            if [1, 3].contains(&v) {
                self.swings.push(3);
            }
            if [0, 1].contains(&v) {
                self.swings.push(12);
            }
            if v == 1 {
                self.swings.push(15);
            }
        }
        let v = self.value(0x210);
        self.custom_fan = v == Some(1);
        self.fans = match v {
            None | Some(1) => vec![20, 40, 60, 80, 102],
            Some(v) => {
                let mut f = vec![];
                if [6, 9].contains(&v) {
                    f.push(20);
                }
                if [3, 4, 5, 6, 7, 9].contains(&v) {
                    f.push(40);
                }
                if [5, 6, 7].contains(&v) {
                    f.push(60);
                }
                if [3, 4, 5, 6, 7, 9].contains(&v) {
                    f.push(80);
                }
                if [4, 5, 6, 9].contains(&v) {
                    f.push(102);
                }
                f
            }
        };
        if self.custom_fan {
            self.fans.push(100);
        }
        self.eco = self.value(0x212).is_some_and(|v| [1, 2].contains(&v));
        self.boost = self.value(0x21a).is_some_and(|v| [0, 1, 3].contains(&v));
        if let Some(t) = self.raw.get(&0x225).filter(|t| t.len() >= 6) {
            self.min_temp = f64::from(*[t[0], t[2], t[4]].iter().min().unwrap()) / 2.0;
            self.max_temp = f64::from(*[t[1], t[3], t[5]].iter().max().unwrap()) / 2.0;
        }
    }
    pub fn validate(&self, c: &SettingsChange) -> Result<()> {
        if c.power.is_none()
            && c.temperature.is_none()
            && c.mode.is_none()
            && c.fan.is_none()
            && c.swing.is_none()
            && c.vertical_angle.is_none()
            && c.horizontal_angle.is_none()
            && c.boost.is_none()
            && c.eco.is_none()
        {
            return Err(problem("Choose at least one setting."));
        }
        if c.temperature.is_some_and(|v| {
            !v.is_finite() || (v * 2.0).fract() != 0.0 || v < self.min_temp || v > self.max_temp
        }) {
            return Err(problem(format!(
                "Temperature must be {:.1}–{:.1}°C in half-degree steps.",
                self.min_temp, self.max_temp
            )));
        }
        if c.fan.is_some_and(|v| {
            !self.fans.contains(&v) && !(self.custom_fan && (1..=100).contains(&v))
        }) {
            return Err(problem(
                "Choose a supported fan percentage (1–100) or Auto.",
            ));
        }
        if c.mode.is_some_and(|v| !self.modes.contains(&v)) {
            return Err(problem("This AC does not support that mode."));
        }
        if c.swing.is_some_and(|v| !self.swings.contains(&v)) {
            return Err(problem("This AC does not support that swing setting."));
        }
        for (value, supported) in [
            (c.vertical_angle, self.vertical_angle),
            (c.horizontal_angle, self.horizontal_angle),
        ] {
            if value.is_some_and(|v| !supported || ![0, 1, 25, 50, 75, 100].contains(&v)) {
                return Err(problem("This AC does not support that vent position."));
            }
        }
        if c.eco.is_some() && !self.eco {
            return Err(problem("This AC does not support Eco mode."));
        }
        if c.boost.is_some() && !self.boost {
            return Err(problem("This AC does not support Boost."));
        }
        Ok(())
    }
}

// Property IDs, payload format, and indexed positions cross-checked against
// mill1000/midea-msmart msmart/device/AC/{command,device}.py.
pub fn angle_query(caps: &Capabilities, sequence: u8) -> Option<Vec<u8>> {
    let mut p = vec![0xb1, 0];
    for (id, supported) in [(9u8, caps.vertical_angle), (10, caps.horizontal_angle)] {
        if supported {
            p.extend([id, 0]);
            p[1] += 1;
        }
    }
    (p[1] > 0).then(|| frame(3, p, sequence))
}
pub fn angle_command(change: &SettingsChange, sequence: u8) -> Option<Vec<u8>> {
    let mut p = vec![0xb0, 0];
    for (id, value) in [(9u8, change.vertical_angle), (10, change.horizontal_angle)] {
        if let Some(v) = value {
            p.extend([id, 0, 1, v as u8]);
            p[1] += 1;
        }
    }
    (p[1] > 0).then(|| frame(2, p, sequence))
}
pub fn decode_angles(bytes: &[u8]) -> Result<(Option<i32>, Option<i32>)> {
    let p = payload(bytes)?;
    if p.len() < 3 || ![0xb0, 0xb1].contains(&p[0]) {
        return Err(problem("Invalid vent position response."));
    }
    // payload() removes the CRC but retains the trailing message sequence.
    // It must never be mistaken for a property header or value.
    let data_end = p.len() - 1;
    let (mut vertical, mut horizontal) = (None, None);
    let mut offset = 2;
    for _ in 0..p[1] {
        if offset + 4 > data_end {
            return Err(problem("Incomplete vent position response."));
        }
        let id = u16::from_le_bytes([p[offset], p[offset + 1]]);
        let failed = p[offset + 2] & 0x10 != 0;
        let size = p[offset + 3] as usize;
        offset += 4;
        if offset + size > data_end {
            return Err(problem("Incomplete vent position response."));
        }
        if !failed && size > 0 {
            let value = i32::from(p[offset]);
            if [0, 1, 25, 50, 75, 100].contains(&value) {
                match id {
                    9 => vertical = Some(value),
                    10 => horizontal = Some(value),
                    _ => {}
                }
            }
        }
        offset += size;
    }
    Ok((vertical, horizontal))
}
