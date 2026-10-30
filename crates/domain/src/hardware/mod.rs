//! Hardware peripheral protocol engines (ESC/POS, serial scales, barcode printers).

pub mod escpos;

pub use escpos::{
    format_idr, format_pos_receipt, Alignment, EscPosBuilder, ReceiptData, ReceiptLineItem,
    TextSize,
};
