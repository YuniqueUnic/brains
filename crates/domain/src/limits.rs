//! Instance limits (design §1.3, §10.3). Defaults are the design's P0 values;
//! an instance may lower them and publish the result via capabilities.

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_text_bytes: usize,
    pub max_attachment_bytes: usize,
    pub path_max_bytes: usize,
    pub segment_max_bytes: usize,
    pub list_page_default: usize,
    pub list_page_max: usize,
    pub history_page_max: usize,
    pub reindex_page_max: usize,
    /// Upper bound of one literal-scan candidate window (design default 100).
    pub scan_window_max: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_text_bytes: 256 * 1024,
            max_attachment_bytes: 8 * 1024 * 1024,
            path_max_bytes: 240,
            segment_max_bytes: 100,
            list_page_default: 20,
            list_page_max: 100,
            history_page_max: 20,
            reindex_page_max: 5,
            scan_window_max: 100,
        }
    }
}
