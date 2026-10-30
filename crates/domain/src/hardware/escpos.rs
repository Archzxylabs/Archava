//! ESC/POS Binary Thermal Printer Protocol & Receipt Formatter.
//!
//! Generates raw byte commands conforming to Epson standard ESC/POS command specifications.
//! Compatible with network (TCP port 9100), USB (WebUSB/WebHID), and Bluetooth thermal receipt printers.

use serde::{Deserialize, Serialize};

/// Alignment modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// Text font sizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    Normal,
    DoubleHeight,
    DoubleWidth,
    DoubleBoth,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptLineItem {
    pub name: String,
    pub qty: i64,
    pub price_cents: i64,
    pub total_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptData {
    pub merchant_name: String,
    pub branch_name: String,
    pub address: String,
    pub order_no: String,
    pub cashier_name: String,
    pub date_time: String,
    pub items: Vec<ReceiptLineItem>,
    pub subtotal_cents: i64,
    pub tax_ppn_cents: i64,
    pub discount_cents: i64,
    pub grand_total_cents: i64,
    pub payment_method: String,
    pub cash_tendered_cents: i64,
    pub change_due_cents: i64,
    pub qris_payload: Option<String>,
    pub footer_message: String,
}

/// Low-level ESC/POS byte sequence builder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EscPosBuilder {
    buffer: Vec<u8>,
}

impl EscPosBuilder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// `ESC @`: Initialize printer and reset settings to default.
    pub fn init(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x40]);
        self
    }

    /// `ESC a n`: Select justification (0 = Left, 1 = Center, 2 = Right).
    pub fn set_alignment(&mut self, align: Alignment) -> &mut Self {
        let code = match align {
            Alignment::Left => 0x00,
            Alignment::Center => 0x01,
            Alignment::Right => 0x02,
        };
        self.buffer.extend_from_slice(&[0x1B, 0x61, code]);
        self
    }

    /// `ESC E n`: Turn emphasized (bold) mode on/off.
    pub fn set_bold(&mut self, bold: bool) -> &mut Self {
        self.buffer
            .extend_from_slice(&[0x1B, 0x45, if bold { 0x01 } else { 0x00 }]);
        self
    }

    /// `GS ! n`: Select character size.
    pub fn set_text_size(&mut self, size: TextSize) -> &mut Self {
        let code = match size {
            TextSize::Normal => 0x00,
            TextSize::DoubleHeight => 0x01,
            TextSize::DoubleWidth => 0x10,
            TextSize::DoubleBoth => 0x11,
        };
        self.buffer.extend_from_slice(&[0x1D, 0x21, code]);
        self
    }

    /// Appends ASCII/UTF-8 text to the buffer.
    pub fn text(&mut self, text: &str) -> &mut Self {
        self.buffer.extend_from_slice(text.as_bytes());
        self
    }

    /// Appends text followed by line feed `\n` (`0x0A`).
    pub fn text_line(&mut self, text: &str) -> &mut Self {
        self.buffer.extend_from_slice(text.as_bytes());
        self.buffer.push(0x0A);
        self
    }

    /// Prints `count` empty lines (`0x0A`).
    pub fn feed(&mut self, count: u8) -> &mut Self {
        for _ in 0..count {
            self.buffer.push(0x0A);
        }
        self
    }

    /// `GS V m`: Partial cut with one point left uncut.
    pub fn cut_paper(&mut self) -> &mut Self {
        // Feed 3 lines before cutting to clear the print head
        self.feed(3);
        self.buffer.extend_from_slice(&[0x1D, 0x56, 0x42, 0x00]);
        self
    }

    /// `ESC p m t1 t2`: Pulse cash drawer kick-out (Pin 2, 50ms ON, 500ms OFF).
    pub fn open_cash_drawer(&mut self) -> &mut Self {
        self.buffer
            .extend_from_slice(&[0x1B, 0x70, 0x00, 0x19, 0xFA]);
        self
    }

    /// `GS ( k`: Generates standard QR Code Model 2 in ESC/POS native bytes.
    pub fn qr_code(&mut self, content: &str) -> &mut Self {
        let bytes = content.as_bytes();
        let len = bytes.len() + 3;
        let p_l = (len & 0xFF) as u8;
        let p_h = ((len >> 8) & 0xFF) as u8;

        // 1. Model: Model 2 (0x32)
        self.buffer
            .extend_from_slice(&[0x1D, 0x28, 0x6B, 0x04, 0x00, 0x31, 0x41, 0x32, 0x00]);
        // 2. Size: Module size 6 dots
        self.buffer
            .extend_from_slice(&[0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x43, 0x06]);
        // 3. Error Correction: Level M (15%)
        self.buffer
            .extend_from_slice(&[0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x45, 0x31]);
        // 4. Store Data
        self.buffer
            .extend_from_slice(&[0x1D, 0x28, 0x6B, p_l, p_h, 0x31, 0x50, 0x30]);
        self.buffer.extend_from_slice(bytes);
        // 5. Print QR
        self.buffer
            .extend_from_slice(&[0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x51, 0x30]);
        self.feed(1);
        self
    }

    /// Renders a two-column line formatted to a fixed character width (default 32 chars for 58mm, 48 chars for 80mm).
    pub fn two_column_line(&mut self, left: &str, right: &str, total_width: usize) -> &mut Self {
        let left_len = left.chars().count();
        let right_len = right.chars().count();

        if left_len + right_len >= total_width {
            self.text_line(left);
            self.set_alignment(Alignment::Right);
            self.text_line(right);
            self.set_alignment(Alignment::Left);
        } else {
            let spaces = total_width - left_len - right_len;
            self.text(left);
            for _ in 0..spaces {
                self.buffer.push(0x20); // space
            }
            self.text_line(right);
        }
        self
    }

    /// Returns the compiled raw bytes ready to send over network socket or serial port.
    pub fn build(self) -> Vec<u8> {
        self.buffer
    }
}

/// Helper to format minor cents into IDR currency string (e.g. 5000000 -> "Rp 50.000").
pub fn format_idr(cents: i64) -> String {
    let rupiah = cents / 100;
    let s = rupiah.to_string();
    let mut result = String::new();

    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push('.');
        }
        result.push(c);
    }

    format!("Rp {}", result.chars().rev().collect::<String>())
}

/// Renders a complete POS thermal receipt into ESC/POS binary format.
pub fn format_pos_receipt(receipt: &ReceiptData, width: usize) -> Vec<u8> {
    let mut b = EscPosBuilder::new();
    b.init();

    // Header
    b.set_alignment(Alignment::Center);
    b.set_text_size(TextSize::DoubleHeight);
    b.set_bold(true);
    b.text_line(&receipt.merchant_name);

    b.set_text_size(TextSize::Normal);
    b.set_bold(false);
    b.text_line(&receipt.branch_name);
    b.text_line(&receipt.address);
    b.feed(1);

    // Order Info
    b.set_alignment(Alignment::Left);
    let sep = "-".repeat(width);
    b.text_line(&sep);
    b.two_column_line(
        &format!("No: {}", receipt.order_no),
        &receipt.date_time,
        width,
    );
    b.text_line(&format!("Kasir: {}", receipt.cashier_name));
    b.text_line(&sep);

    // Items
    for item in &receipt.items {
        b.text_line(&item.name);
        let detail = format!("  {}x @ {}", item.qty, format_idr(item.price_cents));
        let total = format_idr(item.total_cents);
        b.two_column_line(&detail, &total, width);
    }
    b.text_line(&sep);

    // Totals
    b.two_column_line("Subtotal", &format_idr(receipt.subtotal_cents), width);
    if receipt.discount_cents > 0 {
        b.two_column_line(
            "Diskon",
            &format!("-{}", format_idr(receipt.discount_cents)),
            width,
        );
    }
    b.two_column_line("PPN 11%", &format_idr(receipt.tax_ppn_cents), width);

    b.set_bold(true);
    b.set_text_size(TextSize::DoubleHeight);
    b.two_column_line("TOTAL", &format_idr(receipt.grand_total_cents), width);
    b.set_bold(false);
    b.set_text_size(TextSize::Normal);
    b.text_line(&sep);

    // Payment Tender
    b.two_column_line(
        &format!("Bayar ({})", receipt.payment_method),
        &format_idr(receipt.cash_tendered_cents),
        width,
    );
    b.two_column_line("Kembali", &format_idr(receipt.change_due_cents), width);
    b.feed(1);

    // QRIS QR Code if present
    if let Some(qris) = &receipt.qris_payload {
        b.set_alignment(Alignment::Center);
        b.text_line("Scan QRIS untuk Verifikasi:");
        b.qr_code(qris);
    }

    // Footer
    b.set_alignment(Alignment::Center);
    b.text_line(&receipt.footer_message);
    b.text_line("Powered by Archava Platform");

    // Open cash drawer & Cut paper
    b.open_cash_drawer();
    b.cut_paper();

    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escpos_builder_init_and_formatting() {
        let mut b = EscPosBuilder::new();
        b.init()
            .set_alignment(Alignment::Center)
            .set_bold(true)
            .text_line("Kopi Susu Jaksel")
            .open_cash_drawer()
            .cut_paper();

        let bytes = b.build();
        // Check ESC @ prefix
        assert_eq!(&bytes[0..2], &[0x1B, 0x40]);
        // Check Alignment Center
        assert_eq!(&bytes[2..5], &[0x1B, 0x61, 0x01]);
        // Check Bold On
        assert_eq!(&bytes[5..8], &[0x1B, 0x45, 0x01]);
        // Contains text
        assert!(bytes
            .windows("Kopi Susu Jaksel".len())
            .any(|w| w == b"Kopi Susu Jaksel"));
        // Cash drawer pulse present
        assert!(bytes
            .windows(5)
            .any(|w| w == [0x1B, 0x70, 0x00, 0x19, 0xFA]));
        // Cut paper present
        assert!(bytes.windows(4).any(|w| w == [0x1D, 0x56, 0x42, 0x00]));
    }

    #[test]
    fn test_format_pos_receipt_complete_output() {
        let receipt = ReceiptData {
            merchant_name: "Archava Cafe & Bistro".into(),
            branch_name: "Cabang Senopati".into(),
            address: "Jl. Senopati No. 42, Jakarta Selatan".into(),
            order_no: "POS-20261028-0042".into(),
            cashier_name: "Budi".into(),
            date_time: "2026-10-28 14:30".into(),
            items: vec![
                ReceiptLineItem {
                    name: "Iced Latte Arabica".into(),
                    qty: 2,
                    price_cents: 35_000_00,
                    total_cents: 70_000_00,
                },
                ReceiptLineItem {
                    name: "Almond Croissant".into(),
                    qty: 1,
                    price_cents: 45_000_00,
                    total_cents: 45_000_00,
                },
            ],
            subtotal_cents: 115_000_00,
            tax_ppn_cents: 12_650_00,
            discount_cents: 0,
            grand_total_cents: 127_650_00,
            payment_method: "QRIS BCA".into(),
            cash_tendered_cents: 127_650_00,
            change_due_cents: 0,
            qris_payload: Some("00020101021226600016ID.CO.QRIS.WWW0118936005230000000001".into()),
            footer_message: "Terima Kasih Atas Kunjungan Anda!".into(),
        };

        let raw_bytes = format_pos_receipt(&receipt, 48);
        assert!(!raw_bytes.is_empty());
        assert!(raw_bytes
            .windows("Archava Cafe".len())
            .any(|w| w == b"Archava Cafe"));
        assert!(raw_bytes
            .windows("Iced Latte Arabica".len())
            .any(|w| w == b"Iced Latte Arabica"));
    }
}
