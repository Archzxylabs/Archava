//! Computer Use Screen and Browser Automation Actions.

use serde::{Deserialize, Serialize};

/// Atomic UI action executed within the sandboxed headless browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RpaAction {
    /// Navigate to a verified allowed URL.
    Navigate { url: String },
    /// Click an element identified by CSS selector or XPath.
    Click { selector: String },
    /// Type text into an input field.
    TypeText {
        selector: String,
        text: String,
        is_sensitive: bool,
    },
    /// Scrape tabular structured data from an HTML table.
    ExtractTable { table_selector: String },
    /// Capture screenshot for visual verification and audit trail.
    CaptureScreenshot { label: String },
    /// Wait for DOM element readiness.
    WaitForSelector {
        selector: String,
        timeout_millis: u64,
    },
}

/// Execution outcome of an RPA step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpaStepResult {
    pub step_index: usize,
    pub success: bool,
    pub output_summary: String,
    pub extracted_data: Option<serde_json::Value>,
}
