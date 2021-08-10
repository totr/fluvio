//!
//! # Consume CLI
//!
//! CLI command for Consume operation
//!

use std::{io::Error as IoError, path::PathBuf};
use std::io::ErrorKind;
use std::convert::TryFrom;
use tracing::{debug, trace, instrument};
use structopt::StructOpt;
use structopt::clap::arg_enum;
use fluvio_future::io::StreamExt;

mod record_format;

use fluvio::{Fluvio, PartitionConsumer, Offset, ConsumerConfig, FluvioError};
use fluvio_sc_schema::ApiError;
use fluvio::consumer::Record;

use crate::{Result, CliError};
use crate::common::FluvioExtensionMetadata;
use self::record_format::{
    format_text_record, format_binary_record, format_dynamic_record, format_raw_record, format_json,
};

/// Read messages from a topic/partition
///
/// By default, consume operates in "streaming" mode, where the command will remain
/// active and wait for new messages, printing them as they arrive. You can use the
/// '-d' flag to exit after consuming all available messages.
#[derive(Debug, StructOpt)]
pub struct ConsumeOpt {
    /// Topic name
    #[structopt(value_name = "topic")]
    pub topic: String,

    /// Partition id
    #[structopt(short = "p", long, default_value = "0", value_name = "integer")]
    pub partition: i32,

    /// Start reading from beginning
    #[structopt(short = "B", long = "from-beginning")]
    pub from_beginning: bool,

    /// disable continuous processing of messages
    #[structopt(short = "d", long)]
    pub disable_continuous: bool,

    /// Print records in "[key] value" format, with "[null]" for no key
    #[structopt(short, long)]
    pub key_value: bool,

    /// Offsets can be positive or negative. (Syntax for negative offset: --offset="-1")
    #[structopt(short, long, value_name = "integer")]
    pub offset: Option<i64>,

    /// Maximum number of bytes to be retrieved
    #[structopt(short = "b", long = "maxbytes", value_name = "integer")]
    pub max_bytes: Option<i32>,

    /// Suppress items items that have an unknown output type
    #[structopt(long = "suppress-unknown")]
    pub suppress_unknown: bool,

    /// Output
    #[structopt(
        short = "O",
        long = "output",
        value_name = "type",
        possible_values = &ConsumeOutputType::variants(),
        case_insensitive = true,
        default_value
    )]
    pub output: ConsumeOutputType,

    /// Path to a SmartStream filter wasm file
    #[structopt(long, group("smartstream"))]
    pub filter: Option<PathBuf>,

    /// Path to a SmartStream map wasm file
    #[structopt(long, group("smartstream"))]
    pub map: Option<PathBuf>,
}

impl ConsumeOpt {
    #[instrument(
        skip(self, fluvio),
        name = "Consume",
        fields(topic = %self.topic, partition = self.partition),
    )]
    pub async fn process(self, fluvio: &Fluvio) -> Result<()> {
        let consumer = fluvio
            .partition_consumer(&self.topic, self.partition)
            .await?;
        self.consume_records(consumer).await?;
        Ok(())
    }

    pub fn metadata() -> FluvioExtensionMetadata {
        FluvioExtensionMetadata {
            title: "consume".into(),
            package: Some("fluvio/fluvio".parse().unwrap()),
            description: "Consume new data in a stream".into(),
            version: semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
        }
    }

    pub async fn consume_records(&self, consumer: PartitionConsumer) -> Result<()> {
        trace!(config = ?self, "Starting consumer:");
        self.init_ctrlc()?;
        let offset = self.calculate_offset()?;

        let mut builder = ConsumerConfig::builder();
        if let Some(max_bytes) = self.max_bytes {
            builder.max_bytes(max_bytes);
        }

        if let Some(filter_path) = &self.filter {
            let buffer = std::fs::read(filter_path)?;
            debug!(len = buffer.len(), "read filter bytes");
            builder.wasm_filter(buffer);
        }

        if let Some(map_path) = &self.map {
            let buffer = std::fs::read(map_path)?;
            debug!(len = buffer.len(), "read filter bytes");
            builder.wasm_map(buffer);
        }

        let consume_config = builder.build()?;
        if self.disable_continuous {
            self.consume_records_batch(&consumer, offset, consume_config)
                .await?;
        } else {
            self.consume_records_stream(&consumer, offset, consume_config)
                .await?;
        }

        Ok(())
    }

    /// Consume records in a single batch, then exit
    async fn consume_records_batch(
        &self,
        consumer: &PartitionConsumer,
        offset: Offset,
        config: ConsumerConfig,
    ) -> Result<()> {
        let response = consumer.fetch_with_config(offset, config).await?;

        debug!(
            "got a single response: LSO: {} batches: {}",
            response.log_start_offset,
            response.records.batches.len(),
        );

        for batch in response.records.batches.iter() {
            for record in batch.records().iter() {
                let key = record.key.as_ref().map(|it| it.as_ref());
                self.print_record(key, record.value.as_ref());
            }
        }
        Ok(())
    }

    /// Consume records as a stream, waiting for new records to arrive
    async fn consume_records_stream(
        &self,
        consumer: &PartitionConsumer,
        offset: Offset,
        config: ConsumerConfig,
    ) -> Result<()> {
        self.print_status();
        let mut stream = consumer.stream_with_config(offset, config).await?;

        while let Some(result) = stream.next().await {
            let result: std::result::Result<Record, _> = result;
            let record = match result {
                Ok(record) => record,
                Err(FluvioError::AdminApi(ApiError::Code(code, _))) => {
                    eprintln!("{}", code.to_sentence());
                    continue;
                }
                Err(other) => return Err(other.into()),
            };

            self.print_record(record.key(), record.value());
        }

        debug!("fetch loop exited");
        eprintln!("Consumer stream has closed");
        Ok(())
    }

    /// Process fetch topic response based on output type
    pub fn print_record(&self, key: Option<&[u8]>, value: &[u8]) {
        let formatted_key = key.map(|key| {
            String::from_utf8(key.to_owned())
                .unwrap_or_else(|_| "<cannot print non-UTF8 key>".to_string())
        });

        let formatted_value = match self.output {
            ConsumeOutputType::json => format_json(value, self.suppress_unknown),
            ConsumeOutputType::text => Some(format_text_record(value, self.suppress_unknown)),
            ConsumeOutputType::binary => Some(format_binary_record(value)),
            ConsumeOutputType::dynamic => Some(format_dynamic_record(value)),
            ConsumeOutputType::raw => Some(format_raw_record(value)),
        };

        match (formatted_key, formatted_value) {
            (Some(key), Some(value)) if self.key_value => {
                println!("[{}] {}", key, value);
            }
            (None, Some(value)) if self.key_value => {
                println!("[null] {}", value);
            }
            (_, Some(value)) => {
                println!("{}", value);
            }
            // (Some(_), None) only if JSON cannot be printed, so skip.
            _ => debug!("Skipping record that cannot be formatted"),
        }
    }

    fn print_status(&self) {
        use colored::*;
        if !atty::is(atty::Stream::Stdout) {
            return;
        }

        // If -B or --beginning
        if self.from_beginning {
            eprintln!(
                "{}",
                format!(
                    "Consuming records from the beginning of topic '{}'",
                    &self.topic
                )
                .bold()
            );
            return;
        }

        // If -o or --offset was given
        if let Some(offset) = &self.offset {
            if *offset > 0 {
                eprintln!(
                    "{}",
                    format!(
                        "Consuming records from offset {} in topic '{}'",
                        offset, &self.topic
                    )
                    .bold()
                );
            } else {
                eprintln!(
                    "{}",
                    format!(
                        "Consuming records starting {} from the end of topic '{}'",
                        -offset, &self.topic
                    )
                    .bold()
                );
            }
            return;
        }

        // If no offset config is given, read from the end
        eprintln!(
            "{}",
            format!(
                "Consuming records from the end of topic '{}'. This will wait for new records",
                &self.topic
            )
            .bold()
        )
    }

    /// Initialize Ctrl-C event handler
    fn init_ctrlc(&self) -> Result<()> {
        let result = ctrlc::set_handler(move || {
            debug!("detected control c, setting end");
            std::process::exit(0);
        });

        if let Err(err) = result {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!("CTRL-C handler can't be initialized {}", err),
            )
            .into());
        }
        Ok(())
    }

    /// Calculate the Offset to use with the consumer based on the provided offset number
    fn calculate_offset(&self) -> Result<Offset> {
        let maybe_initial_offset = if self.from_beginning {
            let big_offset = self.offset.unwrap_or(0);
            // Try to convert to u32
            u32::try_from(big_offset).ok().map(Offset::from_beginning)
        } else if let Some(big_offset) = self.offset {
            // if it is negative, we start from end
            if big_offset < 0 {
                // Try to convert to u32
                u32::try_from(-big_offset).ok().map(Offset::from_end)
            } else {
                Offset::absolute(big_offset).ok()
            }
        } else {
            Some(Offset::end())
        };

        let offset = maybe_initial_offset
            .ok_or_else(|| CliError::InvalidArg("Illegal offset. Relative offsets must be u32 and absolute offsets must be positive".to_string()))?;
        Ok(offset)
    }
}

// Uses clap::arg_enum to choose possible variables
arg_enum! {
    #[derive(Debug, Clone, PartialEq)]
    #[allow(non_camel_case_types)]
    pub enum ConsumeOutputType {
        dynamic,
        text,
        binary,
        json,
        raw,
    }
}

/// Consume output type defaults to text formatting
impl ::std::default::Default for ConsumeOutputType {
    fn default() -> Self {
        ConsumeOutputType::dynamic
    }
}