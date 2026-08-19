use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct SparseAxisGeometry {
    default_size: f64,
    overrides: Vec<AxisOverride>,
    prefix_delta: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AxisOverride {
    index: usize,
    size: f64,
}

impl SparseAxisGeometry {
    pub fn new(default_size: f64, sizes: &HashMap<usize, u32>) -> Self {
        let mut overrides = sizes
            .iter()
            .map(|(&index, &size)| AxisOverride {
                index,
                size: f64::from(size),
            })
            .collect::<Vec<_>>();
        overrides.sort_unstable_by_key(|entry| entry.index);

        let mut accumulated = 0.0;
        let prefix_delta = overrides
            .iter()
            .map(|entry| {
                accumulated += entry.size - default_size;
                accumulated
            })
            .collect();

        Self {
            default_size,
            overrides,
            prefix_delta,
        }
    }

    pub fn size(&self, index: usize) -> f64 {
        self.overrides
            .binary_search_by_key(&index, |entry| entry.index)
            .ok()
            .map(|position| self.overrides[position].size)
            .unwrap_or(self.default_size)
    }

    pub fn offset(&self, index: usize) -> f64 {
        let overrides_before = self.overrides.partition_point(|entry| entry.index < index);
        let delta = overrides_before
            .checked_sub(1)
            .map(|position| self.prefix_delta[position])
            .unwrap_or(0.0);
        index as f64 * self.default_size + delta
    }

    pub fn index_at(&self, offset: f64, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        let offset = offset.max(0.0);
        let mut low = 0usize;
        let mut high = count;
        while low < high {
            let middle = low + (high - low) / 2;
            if self.offset(middle) <= offset {
                low = middle.saturating_add(1);
            } else {
                high = middle;
            }
        }
        low.saturating_sub(1).min(count.saturating_sub(1))
    }

    pub fn range_for_pixels(&self, start: f64, end: f64, count: usize) -> (usize, usize) {
        if count == 0 {
            return (0, 0);
        }
        let start_index = self.index_at(start, count);
        let end_index = self
            .index_at(end.max(start), count)
            .saturating_add(1)
            .min(count);
        (start_index, end_index.max(start_index.saturating_add(1)))
    }

    pub fn total_size(&self, count: usize) -> f64 {
        self.offset(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_offsets_and_lookup_are_inverse() {
        let geometry = SparseAxisGeometry::new(120.0, &HashMap::from([(1, 200), (4, 80)]));

        assert_eq!(geometry.offset(3), 440.0);
        assert_eq!(geometry.index_at(441.0, 10), 3);
        assert_eq!(geometry.size(1), 200.0);
        assert_eq!(geometry.total_size(10), 1_240.0);
    }

    #[test]
    fn pixel_range_is_clamped_and_end_exclusive() {
        let geometry = SparseAxisGeometry::new(30.0, &HashMap::new());

        assert_eq!(geometry.range_for_pixels(35.0, 89.0, 5), (1, 3));
        assert_eq!(geometry.range_for_pixels(-20.0, 1_000.0, 5), (0, 5));
        assert_eq!(geometry.range_for_pixels(0.0, 10.0, 0), (0, 0));
    }
}
