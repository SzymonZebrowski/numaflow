/// Jetstream ISB related configurations.
use std::fmt;
use std::time::Duration;

const DEFAULT_PARTITION_IDX: u16 = 0;
const DEFAULT_PARTITIONS: u16 = 1;
const DEFAULT_MAX_LENGTH: usize = 30000;
const DEFAULT_USAGE_LIMIT: f64 = 0.8;
const DEFAULT_REFRESH_INTERVAL_SECS: u64 = 1;
const DEFAULT_BUFFER_FULL_STRATEGY: BufferFullStrategy = BufferFullStrategy::RetryUntilSuccess;
const DEFAULT_RETRY_INTERVAL_MILLIS: u64 = 10;
const DEFAULT_WIP_ACK_INTERVAL_MILLIS: u64 = 1000;

pub(crate) mod jetstream {
    const DEFAULT_URL: &str = "localhost:4222";
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct ClientConfig {
        pub url: String,
        pub user: Option<String>,
        pub password: Option<String>,
    }

    impl Default for ClientConfig {
        fn default() -> Self {
            ClientConfig {
                url: DEFAULT_URL.to_string(),
                user: None,
                password: None,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BufferWriterConfig {
    pub streams: Vec<(String, u16)>,
    pub partitions: u16,
    pub max_length: usize,
    pub refresh_interval: Duration,
    pub usage_limit: f64,
    pub buffer_full_strategy: BufferFullStrategy,
    pub retry_interval: Duration,
}

impl Default for BufferWriterConfig {
    fn default() -> Self {
        BufferWriterConfig {
            streams: vec![("default-0".to_string(), DEFAULT_PARTITION_IDX)],
            partitions: DEFAULT_PARTITIONS,
            max_length: DEFAULT_MAX_LENGTH,
            usage_limit: DEFAULT_USAGE_LIMIT,
            refresh_interval: Duration::from_secs(DEFAULT_REFRESH_INTERVAL_SECS),
            buffer_full_strategy: DEFAULT_BUFFER_FULL_STRATEGY,
            retry_interval: Duration::from_millis(DEFAULT_RETRY_INTERVAL_MILLIS),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BufferFullStrategy {
    RetryUntilSuccess,
    #[allow(dead_code)]
    DiscardLatest,
}

impl fmt::Display for BufferFullStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BufferFullStrategy::RetryUntilSuccess => write!(f, "retryUntilSuccess"),
            BufferFullStrategy::DiscardLatest => write!(f, "discardLatest"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BufferReaderConfig {
    pub(crate) partitions: u16,
    pub(crate) streams: Vec<(&'static str, u16)>,
    pub(crate) wip_ack_interval: Duration,
}

impl Default for BufferReaderConfig {
    fn default() -> Self {
        BufferReaderConfig {
            partitions: DEFAULT_PARTITIONS,
            streams: vec![("default-0", DEFAULT_PARTITION_IDX)],
            wip_ack_interval: Duration::from_millis(DEFAULT_WIP_ACK_INTERVAL_MILLIS),
        }
    }
}

#[cfg(test)]
mod jetstream_client_config {
    use super::jetstream::*;

    #[test]
    fn test_default_client_config() {
        let expected_config = ClientConfig {
            url: "localhost:4222".to_string(),
            user: None,
            password: None,
        };
        let config = ClientConfig::default();
        assert_eq!(config, expected_config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_buffer_writer_config() {
        let expected = BufferWriterConfig {
            streams: vec![("default-0".to_string(), DEFAULT_PARTITION_IDX)],
            partitions: DEFAULT_PARTITIONS,
            max_length: DEFAULT_MAX_LENGTH,
            usage_limit: DEFAULT_USAGE_LIMIT,
            refresh_interval: Duration::from_secs(DEFAULT_REFRESH_INTERVAL_SECS),
            buffer_full_strategy: DEFAULT_BUFFER_FULL_STRATEGY,
            retry_interval: Duration::from_millis(DEFAULT_RETRY_INTERVAL_MILLIS),
        };
        let config = BufferWriterConfig::default();

        assert_eq!(config, expected);
    }

    #[test]
    fn test_buffer_full_strategy_display() {
        let val = BufferFullStrategy::RetryUntilSuccess;
        assert_eq!(val.to_string(), "retryUntilSuccess");

        let val = BufferFullStrategy::DiscardLatest;
        assert_eq!(val.to_string(), "discardLatest");
    }

    #[test]
    fn test_default_buffer_reader_config() {
        let expected = BufferReaderConfig {
            partitions: DEFAULT_PARTITIONS,
            streams: vec![("default-0", DEFAULT_PARTITION_IDX)],
            wip_ack_interval: Duration::from_millis(DEFAULT_WIP_ACK_INTERVAL_MILLIS),
        };
        let config = BufferReaderConfig::default();
        assert_eq!(config, expected);
    }
}
