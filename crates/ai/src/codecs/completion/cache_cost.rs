use std::iter::Sum;
use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};

use super::Usage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CacheRates {
    pub input: f64,
    pub cached_read: f64,
    pub cache_write: f64,
    pub storage_per_hour: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CacheCost {
    pub uncached_input: u64,
    pub cache_reads: u64,
    pub cache_writes: u64,
    pub storage_token_hours: f64,
}

impl CacheCost {
    pub fn from_usage(usage: &Usage) -> Self {
        let input = usage.input_tokens.unwrap_or(0);
        let cache_reads = usage.cached_input_tokens.unwrap_or(0);
        let cache_writes = usage.cache_creation_input_tokens.unwrap_or(0);
        Self {
            uncached_input: input.saturating_sub(cache_reads + cache_writes),
            cache_reads,
            cache_writes,
            storage_token_hours: 0.0,
        }
    }

    pub fn prompt_tokens(&self) -> u64 {
        self.uncached_input + self.cache_reads + self.cache_writes
    }

    pub fn usd(&self, rates: &CacheRates) -> f64 {
        (self.uncached_input as f64 * rates.input
            + self.cache_reads as f64 * rates.cached_read
            + self.cache_writes as f64 * rates.cache_write
            + self.storage_token_hours * rates.storage_per_hour)
            / 1e6
    }

    pub fn uncached_usd(&self, rates: &CacheRates) -> f64 {
        self.prompt_tokens() as f64 * rates.input / 1e6
    }

    pub fn saving(&self, rates: &CacheRates) -> f64 {
        let uncached = self.uncached_usd(rates);
        if uncached > 0.0 {
            1.0 - self.usd(rates) / uncached
        } else {
            0.0
        }
    }
}

impl Add for CacheCost {
    type Output = Self;

    fn add(mut self, other: Self) -> Self {
        self += other;
        self
    }
}

impl AddAssign for CacheCost {
    fn add_assign(&mut self, other: Self) {
        self.uncached_input += other.uncached_input;
        self.cache_reads += other.cache_reads;
        self.cache_writes += other.cache_writes;
        self.storage_token_hours += other.storage_token_hours;
    }
}

impl Sum for CacheCost {
    fn sum<I: Iterator<Item = Self>>(costs: I) -> Self {
        costs.fold(Self::default(), Add::add)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    const RATES: CacheRates = CacheRates {
        input: 4.0,
        cached_read: 0.2,
        cache_write: 5.0,
        storage_per_hour: 1.0,
    };

    fn usage(input: u64, cached: Option<u64>, written: Option<u64>) -> Usage {
        Usage {
            input_tokens: Some(input),
            cached_input_tokens: cached,
            cache_creation_input_tokens: written,
            ..Usage::default()
        }
    }

    #[test]
    fn absent_and_inconsistent_counters() {
        let cost = CacheCost::from_usage(&Usage::default());
        assert_eq!(cost, CacheCost::default());
        assert!(cost.saving(&RATES).abs() < f64::EPSILON);
        let cost = CacheCost::from_usage(&usage(100, Some(200), None));
        assert_eq!(cost.uncached_input, 0);
    }

    #[test]
    fn prices_every_part_at_its_rate() {
        let cost = CacheCost {
            uncached_input: 1_000_000,
            cache_reads: 2_000_000,
            cache_writes: 1_000_000,
            storage_token_hours: 500_000.0,
        };
        let usd = cost.usd(&RATES);
        assert!((usd - (4.0 + 0.4 + 5.0 + 0.5)).abs() < 1e-12, "{usd}");
        let uncached = cost.uncached_usd(&RATES);
        assert!((uncached - 16.0).abs() < 1e-12, "{uncached}");
        let saving = cost.saving(&RATES);
        assert!((saving - (1.0 - 9.9 / 16.0)).abs() < 1e-12, "{saving}");
    }

    #[test]
    fn sums_a_run() {
        let calls = [
            CacheCost::from_usage(&usage(1_000, Some(0), Some(990))),
            CacheCost::from_usage(&usage(1_050, Some(990), Some(50))),
        ];
        let storage = CacheCost {
            storage_token_hours: 2.5,
            ..CacheCost::default()
        };
        let run: CacheCost = calls.into_iter().chain([storage]).sum();
        assert_eq!(
            run,
            CacheCost {
                uncached_input: 20,
                cache_reads: 990,
                cache_writes: 1_040,
                storage_token_hours: 2.5,
            }
        );
    }
}
