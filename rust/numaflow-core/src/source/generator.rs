use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;

use crate::message::{
    get_vertex_name, get_vertex_replica, Message, MessageID, Offset, StringOffset,
};
use crate::reader;
use crate::source;

/// Stream Generator returns a set of messages for every `.next` call. It will throttle itself if
/// the call exceeds the RPU. It will return a max (batch size, RPU) till the quota for that unit of
/// time is over. If `.next` is called after the quota is over, it will park itself so that it won't
/// return more than the RPU. Once parked, it will unpark itself and return as soon as the next poll
/// happens.
/// We skip the missed ticks because there is no point to give a burst, most likely that burst cannot
/// be absorbed.
/// ```text
///       Ticks: |     1     |     2     |     3     |     4     |     5     |     6     |
///              =========================================================================> time
///  Read RPU=5: | :xxx:xx:  | :xxx <delay>             |:xxx:xx:| :xxx:xx:  | :xxx:xx:  |
///                2 batches   only 1 batch (no reread)      5         5           5
///                 
/// ```
/// NOTE: The minimum granularity of duration is 10ms.
mod stream_generator {
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use bytes::Bytes;
    use futures::Stream;
    use pin_project::pin_project;
    use tokio::time::MissedTickBehavior;

    #[pin_project]
    pub(super) struct StreamGenerator {
        /// the content generated by Generator.
        content: Bytes,
        /// requests per unit of time-period.
        rpu: usize,
        /// batch size per read
        batch: usize,
        /// the amount of credits used for the current time-period.
        /// remaining = (rpu - used) for that time-period
        used: usize,
        #[pin]
        tick: tokio::time::Interval,
    }

    impl StreamGenerator {
        pub(super) fn new(content: Bytes, rpu: usize, batch: usize, unit: Duration) -> Self {
            let mut tick = tokio::time::interval(unit);
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

            Self {
                content,
                rpu,
                // batch cannot > rpu
                batch: if batch > rpu { rpu } else { batch },
                used: 0,
                tick,
            }
        }
    }

    impl Stream for StreamGenerator {
        type Item = Vec<Bytes>;

        fn poll_next(
            mut self: Pin<&mut StreamGenerator>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            let mut this = self.as_mut().project();

            match this.tick.poll_tick(cx) {
                // Poll::Ready means we are ready to send data the whole batch since enough time
                // has passed.
                Poll::Ready(_) => {
                    // generate data that equals to batch data
                    let data = vec![this.content.clone(); *this.batch];
                    // reset used quota
                    *this.used = *this.batch;

                    Poll::Ready(Some(data))
                }
                Poll::Pending => {
                    // even if enough time hasn't passed, we can still send data if we have
                    // quota (rpu - used) left
                    if this.used < this.rpu {
                        // make sure we do not send more than desired
                        let to_send = std::cmp::min(*this.rpu - *this.used, *this.batch);

                        // update the counters
                        *this.used += to_send;

                        Poll::Ready(Some(vec![this.content.clone(); to_send]))
                    } else {
                        Poll::Pending
                    }
                }
            }
        }

        /// size is roughly what is remaining and upper bound is for sure RPU. This is a very
        /// rough approximation because Duration is not taken into account for the lower bound.
        fn size_hint(&self) -> (usize, Option<usize>) {
            (self.rpu - self.used, Some(self.rpu))
        }
    }

    #[cfg(test)]
    mod tests {
        use futures::StreamExt;

        use super::*;

        #[tokio::test]
        async fn test_stream_generator() {
            // Define the content to be generated
            let content = Bytes::from("test_data");
            // Define requests per unit (rpu), batch size, and time unit
            let rpu = 10;
            let batch = 6;
            let unit = Duration::from_millis(100);

            // Create a new StreamGenerator
            let mut stream_generator = StreamGenerator::new(content.clone(), rpu, batch, unit);

            // Collect the first batch of data
            let first_batch = stream_generator.next().await.unwrap();
            assert_eq!(first_batch.len(), batch);
            for item in first_batch {
                assert_eq!(item, content);
            }

            // Collect the second batch of data
            let second_batch = stream_generator.next().await.unwrap();
            assert_eq!(second_batch.len(), rpu - batch);
            for item in second_batch {
                assert_eq!(item, content);
            }

            // no there is no more data left in the quota
            let size = stream_generator.size_hint();
            assert_eq!(size.0, 0);
            assert_eq!(size.1, Some(rpu));

            let third_batch = stream_generator.next().await.unwrap();
            assert_eq!(third_batch.len(), 6);
            for item in third_batch {
                assert_eq!(item, content);
            }

            // we should now have data
            let size = stream_generator.size_hint();
            assert_eq!(size.0, 4);
            assert_eq!(size.1, Some(rpu));
        }
    }
}

/// Creates a new generator and returns all the necessary implementation of the Source trait.
/// Generator Source is mainly used for development purpose, where you want to have self-contained
/// source to generate some messages. We mainly use generator for load testing and integration
/// testing of Numaflow. The load generated is per replica.
pub(crate) fn new_generator(
    content: Bytes,
    rpu: usize,
    batch: usize,
    unit: Duration,
) -> crate::Result<(GeneratorRead, GeneratorAck, GeneratorLagReader)> {
    let gen_read = GeneratorRead::new(content, rpu, batch, unit);
    let gen_ack = GeneratorAck::new();
    let gen_lag_reader = GeneratorLagReader::new();

    Ok((gen_read, gen_ack, gen_lag_reader))
}

pub(crate) struct GeneratorRead {
    stream_generator: stream_generator::StreamGenerator,
}

impl GeneratorRead {
    /// A new [GeneratorRead] is returned. It takes a static content, requests per unit-time, batch size
    /// to return per [source::SourceReader::read], and the unit-time as duration.
    fn new(content: Bytes, rpu: usize, batch: usize, unit: Duration) -> Self {
        let stream_generator = stream_generator::StreamGenerator::new(content, rpu, batch, unit);
        Self { stream_generator }
    }
}

impl source::SourceReader for GeneratorRead {
    fn name(&self) -> &'static str {
        "generator"
    }

    async fn read(&mut self) -> crate::error::Result<Vec<Message>> {
        match self.stream_generator.next().await {
            None => {
                panic!("Stream generator has stopped");
            }
            Some(data) => Ok(data
                .iter()
                .map(|msg| {
                    // FIXME: better id?
                    let id = chrono::Utc::now()
                        .timestamp_nanos_opt()
                        .unwrap_or_default()
                        .to_string();

                    let offset =
                        Offset::String(StringOffset::new(id.clone(), *get_vertex_replica()));

                    Message {
                        keys: vec![],
                        value: msg.clone().to_vec(),
                        // FIXME: better offset?
                        offset: Some(offset.clone()),
                        event_time: Default::default(),
                        id: MessageID {
                            vertex_name: get_vertex_name().to_string(),
                            offset: offset.to_string(),
                            index: Default::default(),
                        },
                        headers: Default::default(),
                    }
                })
                .collect::<Vec<_>>()),
        }
    }

    fn partitions(&self) -> Vec<u16> {
        todo!()
    }
}

pub(crate) struct GeneratorAck {}

impl GeneratorAck {
    fn new() -> Self {
        Self {}
    }
}

impl source::SourceAcker for GeneratorAck {
    async fn ack(&mut self, _: Vec<Offset>) -> crate::error::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct GeneratorLagReader {}

impl GeneratorLagReader {
    fn new() -> Self {
        Self {}
    }
}

impl reader::LagReader for GeneratorLagReader {
    async fn pending(&mut self) -> crate::error::Result<Option<usize>> {
        // Generator is not meant to auto-scale.
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::Duration;

    use super::*;
    use crate::reader::LagReader;
    use crate::source::{SourceAcker, SourceReader};

    #[tokio::test]
    async fn test_generator_read() {
        // Define the content to be generated
        let content = Bytes::from("test_data");
        // Define requests per unit (rpu), batch size, and time unit
        let rpu = 10;
        let batch = 5;
        let unit = Duration::from_millis(100);

        // Create a new Generator
        let mut generator = GeneratorRead::new(content.clone(), rpu, batch, unit);

        // Read the first batch of messages
        let messages = generator.read().await.unwrap();
        assert_eq!(messages.len(), batch);

        // Verify that each message has the expected structure

        // Read the second batch of messages
        let messages = generator.read().await.unwrap();
        assert_eq!(messages.len(), rpu - batch);
    }

    #[tokio::test]
    async fn test_generator_lag_pending() {
        // Create a new GeneratorLagReader
        let mut lag_reader = GeneratorLagReader::new();

        // Call the pending method and check the result
        let pending_result = lag_reader.pending().await;

        // Assert that the result is Ok(None)
        assert!(pending_result.is_ok());
        assert_eq!(pending_result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_generator_ack() {
        // Create a new GeneratorAck instance
        let mut generator_ack = GeneratorAck::new();

        // Create a vector of offsets to acknowledge
        let offsets = vec![
            Offset::String(StringOffset::new("offset1".to_string(), 0)),
            Offset::String(StringOffset::new("offset2".to_string(), 0)),
        ];

        // Call the ack method and check the result
        let ack_result = generator_ack.ack(offsets).await;

        // Assert that the result is Ok(())
        assert!(ack_result.is_ok());
    }
}
