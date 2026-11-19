//! The service omits unavailable entries, so a short page is not the last page.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageOrigin {
    Zero,
    One,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub offset: u64,
    pub page_size: u32,
    pub total: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct PagedCursor {
    origin: PageOrigin,
    page_size: u32,
    offset: u64,
    exhausted: bool,
}

impl PagedCursor {
    pub fn new(origin: PageOrigin, page_size: i32) -> Self {
        Self {
            origin,
            page_size: page_size.clamp(1, 100) as u32,
            offset: 0,
            exhausted: false,
        }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn page_size(&self) -> u32 {
        self.page_size
    }
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }
    pub fn page_number(&self) -> u64 {
        self.offset / u64::from(self.page_size) + u64::from(self.origin == PageOrigin::One)
    }

    /// Call only after a successful request; a failed request must leave the cursor intact.
    pub fn advance(&mut self, received: usize) {
        if self.exhausted {
            return;
        }
        if received == 0 {
            self.exhausted = true;
        } else {
            self.offset = self.offset.saturating_add(u64::from(self.page_size));
        }
    }
}

#[cfg(test)]
#[path = "paging_tests.rs"]
mod tests;
