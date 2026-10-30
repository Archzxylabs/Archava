//! Real-Time IoT Fleet Telematics Protocol Engine.
//!
//! Provides binary frame decoders for enterprise vehicle telematics hardware:
//! 1. Teltonika AVL Codec 8 (FMB920 / FMB120 / FMC130 standard).
//! 2. Concox GT06 binary protocol.
//!
//! Extracts real-time GPS coordinates, velocity, ignition state, fuel levels, and odometer.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub device_imei: String,
    pub timestamp_epoch_ms: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_meters: i16,
    pub angle_heading_degrees: u16,
    pub speed_kmh: u16,
    pub satellites_count: u8,
    pub ignition_on: bool,
    pub fuel_analog_millivolts: Option<u16>,
    pub odometer_meters: Option<u64>,
}

/// Validates that a telemetry record has sane physical bounds.
pub fn validate_telemetry_bounds(rec: &TelemetryRecord) -> Result<(), &'static str> {
    if !(-90.0..=90.0).contains(&rec.latitude) {
        return Err("Latitude out of physical range (-90..90)");
    }
    if !(-180.0..=180.0).contains(&rec.longitude) {
        return Err("Longitude out of physical range (-180..180)");
    }
    if rec.speed_kmh > 250 {
        return Err("Speed exceeds realistic fleet velocity cap (>250 km/h)");
    }
    if rec.angle_heading_degrees >= 360 {
        return Err("Heading angle out of range (0..359 degrees)");
    }
    Ok(())
}

/// Computes the Teltonika CRC-16 checksum (CRC-16/IBM / CRC-16/ARC with polynomial 0xA001).
pub fn teltonika_crc16(bytes: &[u8]) -> u32 {
    let mut crc: u16 = 0;
    for &b in bytes {
        crc ^= b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc as u32
}

/// Decodes a Teltonika AVL Data Packet (Codec 8 Protocol).
///
/// Binary frame layout:
/// - Preamble (4 bytes zeros: 0x00000000)
/// - Data Length (4 bytes big-endian)
/// - Data Field:
///   - Codec ID (1 byte: 0x08 for Codec 8)
///   - Number of Records 1 (1 byte)
///   - Array of AVL Records:
///     - Timestamp (8 bytes UTC ms)
///     - Priority (1 byte)
///     - Longitude (4 bytes signed int / 10,000,000.0)
///     - Latitude (4 bytes signed int / 10,000,000.0)
///     - Altitude (2 bytes signed int)
///     - Angle (2 bytes unsigned int)
///     - Satellites (1 byte)
///     - Speed (2 bytes unsigned int km/h)
///     - IO Elements (IO event ID, total IO count, 1-byte, 2-byte, 4-byte, 8-byte IO elements)
///   - Number of Records 2 (1 byte, repeated count)
/// - CRC-16 (4 bytes big-endian)
pub fn decode_teltonika_codec8(
    device_imei: &str,
    raw_bytes: &[u8],
) -> Result<Vec<TelemetryRecord>, &'static str> {
    if raw_bytes.len() < 16 {
        return Err("Packet too short for Teltonika Codec 8 header");
    }

    // Check Preamble (4 bytes zero)
    if raw_bytes[0..4] != [0x00, 0x00, 0x00, 0x00] {
        return Err("Invalid Teltonika preamble (must be 0x00000000)");
    }

    let data_len =
        u32::from_be_bytes([raw_bytes[4], raw_bytes[5], raw_bytes[6], raw_bytes[7]]) as usize;
    if data_len < 3 {
        return Err("Teltonika data length too short to contain Codec ID and records count");
    }

    // Declared packet framing: 4B preamble + 4B length + data_len + 4B CRC
    let expected_total_len = 8 + data_len + 4;
    if raw_bytes.len() < expected_total_len {
        return Err("Incomplete Teltonika packet (missing trailer or CRC-16)");
    }
    if raw_bytes.len() > expected_total_len {
        return Err("Extraneous bytes past declared Teltonika frame boundary");
    }

    let codec_id = raw_bytes[8];
    if codec_id != 0x08 {
        return Err("Unsupported Teltonika codec (only Codec 8 0x08 is supported)");
    }

    let num_records = raw_bytes[9] as usize;
    if num_records == 0 {
        return Err("Teltonika packet declared zero records");
    }

    // Verify repeated record count (Number of Data 2 at end of Data Field)
    let repeated_records = raw_bytes[8 + data_len - 1] as usize;
    if repeated_records != num_records {
        return Err("Repeated record count mismatch (Number of Data 1 != Number of Data 2)");
    }

    // Verify CRC-16 checksum over Data Field [8 .. 8 + data_len]
    let declared_crc = u32::from_be_bytes([
        raw_bytes[8 + data_len],
        raw_bytes[8 + data_len + 1],
        raw_bytes[8 + data_len + 2],
        raw_bytes[8 + data_len + 3],
    ]);
    let computed_crc = teltonika_crc16(&raw_bytes[8..8 + data_len]);
    if declared_crc != computed_crc {
        return Err("Teltonika CRC-16 checksum mismatch");
    }

    let mut offset = 10;
    let mut records = Vec::with_capacity(num_records);

    for _ in 0..num_records {
        if offset + 24 > raw_bytes.len() {
            return Err("Unexpected end of record buffer");
        }

        // Timestamp (8 bytes)
        let ts = i64::from_be_bytes(
            raw_bytes[offset..offset + 8]
                .try_into()
                .map_err(|_| "Invalid ts")?,
        );
        offset += 8;

        // Priority (1 byte)
        offset += 1;

        // Longitude (4 bytes)
        let lon_raw = i32::from_be_bytes(
            raw_bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid lon")?,
        );
        let lon = lon_raw as f64 / 10_000_000.0;
        offset += 4;

        // Latitude (4 bytes)
        let lat_raw = i32::from_be_bytes(
            raw_bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid lat")?,
        );
        let lat = lat_raw as f64 / 10_000_000.0;
        offset += 4;

        // Altitude (2 bytes)
        let alt = i16::from_be_bytes(
            raw_bytes[offset..offset + 2]
                .try_into()
                .map_err(|_| "Invalid alt")?,
        );
        offset += 2;

        // Angle (2 bytes)
        let angle = u16::from_be_bytes(
            raw_bytes[offset..offset + 2]
                .try_into()
                .map_err(|_| "Invalid angle")?,
        );
        offset += 2;

        // Satellites (1 byte)
        let sats = raw_bytes[offset];
        offset += 1;

        // Speed (2 bytes)
        let speed = u16::from_be_bytes(
            raw_bytes[offset..offset + 2]
                .try_into()
                .map_err(|_| "Invalid speed")?,
        );
        offset += 2;

        // IO Parsing (C07 FIX: Enforce strict framing bounds for all IO elements)
        let mut ignition = false;
        let mut fuel_mv = None;
        let mut odometer_m = None;

        // Minimum IO header is 6 bytes: event_id(1) + total_io(1) + n1(1) + n2(1) + n4(1) + n8(1)
        if offset + 6 > raw_bytes.len() {
            return Err("Truncated telematics frame: missing IO elements header");
        }

        let _event_id = raw_bytes[offset];
        let _total_io = raw_bytes[offset + 1];
        offset += 2;

        // 1-byte IO count
        let n1 = raw_bytes[offset] as usize;
        offset += 1;
        if offset + (n1 * 2) > raw_bytes.len() {
            return Err("Truncated telematics frame: 1-byte IO elements truncated");
        }
        for _ in 0..n1 {
            let io_id = raw_bytes[offset];
            let io_val = raw_bytes[offset + 1];
            offset += 2;
            if io_id == 239 {
                // Teltonika ID 239 = Ignition DIN1
                ignition = io_val == 1;
            }
        }

        // 2-byte IO count
        if offset >= raw_bytes.len() {
            return Err("Truncated telematics frame: missing 2-byte IO count");
        }
        let n2 = raw_bytes[offset] as usize;
        offset += 1;
        if offset + (n2 * 3) > raw_bytes.len() {
            return Err("Truncated telematics frame: 2-byte IO elements truncated");
        }
        for _ in 0..n2 {
            let io_id = raw_bytes[offset];
            let io_val = u16::from_be_bytes([raw_bytes[offset + 1], raw_bytes[offset + 2]]);
            offset += 3;
            if io_id == 9 {
                // Teltonika ID 9 = Analog Input 1 (Fuel sensor mV)
                fuel_mv = Some(io_val);
            }
        }

        // 4-byte IO count
        if offset >= raw_bytes.len() {
            return Err("Truncated telematics frame: missing 4-byte IO count");
        }
        let n4 = raw_bytes[offset] as usize;
        offset += 1;
        if offset + (n4 * 5) > raw_bytes.len() {
            return Err("Truncated telematics frame: 4-byte IO elements truncated");
        }
        for _ in 0..n4 {
            let io_id = raw_bytes[offset];
            let io_val = u32::from_be_bytes([
                raw_bytes[offset + 1],
                raw_bytes[offset + 2],
                raw_bytes[offset + 3],
                raw_bytes[offset + 4],
            ]);
            offset += 5;
            if io_id == 16 {
                // Teltonika ID 16 = Total Odometer in meters
                odometer_m = Some(io_val as u64);
            }
        }

        // 8-byte IO count
        if offset >= raw_bytes.len() {
            return Err("Truncated telematics frame: missing 8-byte IO count");
        }
        let n8 = raw_bytes[offset] as usize;
        offset += 1;
        if offset + (n8 * 9) > raw_bytes.len() {
            return Err("Truncated telematics frame: 8-byte IO elements truncated");
        }
        offset += n8 * 9; // Skip 8-byte IO elements

        let rec = TelemetryRecord {
            device_imei: device_imei.to_string(),
            timestamp_epoch_ms: ts,
            latitude: lat,
            longitude: lon,
            altitude_meters: alt,
            angle_heading_degrees: angle,
            speed_kmh: speed,
            satellites_count: sats,
            ignition_on: ignition,
            fuel_analog_millivolts: fuel_mv,
            odometer_meters: odometer_m,
        };

        if validate_telemetry_bounds(&rec).is_ok() {
            records.push(rec);
        }
    }

    if offset != 8 + data_len - 1 {
        return Err("Record boundaries do not match declared Teltonika data length");
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_teltonika_codec8_valid_frame() {
        // Construct a real Teltonika Codec 8 packet with complete framing:
        // Preamble: 00 00 00 00
        // Data length: 00 00 00 23 (35 bytes: codec(1) + records(1) + avl(32) + repeated_records(1))
        let mut data_field = vec![
            0x08, // Codec 8
            0x01, // 1 record
            // AVL Record (24 bytes)
            0x00, 0x00, 0x01, 0x92, 0xCD, 0x5E, 0x6E, 0x80, // Timestamp
            0x00, // Priority
            0x3F, 0xAF, 0x58, 0x40, // Lon (106.845600)
            0xFC, 0x4C, 0x9E, 0x00, // Lat (-6.208768)
            0x00, 0x1E, // Altitude 30m
            0x00, 0x5A, // Angle 90 deg
            0x0E, // Satellites 14
            0x00, 0x3C, // Speed 60 km/h
            // IO Elements (8 bytes)
            0x00, // Event ID
            0x01, // Total IO
            0x01, // 1-byte count
            0xEF, 0x01, // ID 239 (Ignition) = 1 (ON)
            0x00, // 2-byte count
            0x00, // 4-byte count
            0x00, // 8-byte count
            0x01, // Number of Data 2 (repeated record count)
        ];
        let data_len = data_field.len() as u32;
        let crc = teltonika_crc16(&data_field);

        let mut packet = vec![0x00, 0x00, 0x00, 0x00]; // Preamble
        packet.extend_from_slice(&data_len.to_be_bytes()); // Data length
        packet.append(&mut data_field); // Data Field
        packet.extend_from_slice(&crc.to_be_bytes()); // 4-byte CRC trailer

        let records =
            decode_teltonika_codec8("867530900012345", &packet).expect("Decodes valid frame");
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.device_imei, "867530900012345");
        assert_eq!(r.satellites_count, 14);
        assert_eq!(r.speed_kmh, 60);
        assert!(r.ignition_on);
        assert!((r.latitude - -6.208768).abs() < 0.0001);
        assert!((r.longitude - 106.845600).abs() < 0.0001);
    }

    #[test]
    fn test_c07_truncated_frame_is_rejected() {
        // Codex C07 reproduction: 34-byte zero packet claiming 1 record but missing IO block
        let mut truncated = vec![0u8; 34];
        truncated[7] = 26; // Data length 26
        truncated[8] = 8; // Codec 8
        truncated[9] = 1; // 1 record

        let result = decode_teltonika_codec8("867530900012345", &truncated);
        assert!(
            result.is_err(),
            "Truncated telematics packet must be refused, got: {:?}",
            result
        );
    }

    #[test]
    fn test_r08_codex_probe_40_byte_zero_buffer_is_rejected() {
        // Codex R08 reproduction: 40-byte zero buffer with [7]=32, [8]=8, [9]=1
        // Must be rejected because it lacks repeated record count and valid CRC-16 trailer!
        let mut probe = vec![0u8; 40];
        probe[7] = 32;
        probe[8] = 8;
        probe[9] = 1;

        let result = decode_teltonika_codec8("867530900012345", &probe);
        assert!(
            result.is_err(),
            "Zero probe lacking trailer and CRC must be rejected"
        );
    }
}
