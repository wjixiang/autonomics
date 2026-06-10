//! Streaming support for real-time message generation.
//!
//! This module provides the `MessageStream` struct which handles Server-Sent Events (SSE)
//! from the Anthropic API, accumulates messages from incremental updates, and provides
//! an event-driven API for processing streaming responses.

pub mod events;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use futures::Stream;
use pin_project::pin_project;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::wrappers::BroadcastStream;

use crate::types::{
    Message, MessageStreamEvent, ContentBlock, ContentBlockDelta, 
    AnthropicError, Result
};

use self::events::{EventHandler, EventType};

/// A streaming response from the Anthropic API.
///
/// `MessageStream` provides an event-driven interface for processing streaming responses
/// from Claude. It accumulates message content from incremental updates and provides
/// both callback-based and async iteration APIs.
///
/// # Examples
///
/// ## Callback-based processing:
/// ```ignore
/// # use agentik_sdk::{Anthropic, MessageCreateBuilder};
/// # async fn example() -> agentik_sdk::Result<()> {
/// let client = Anthropic::new("your-api-key")?;
/// let stream = client.messages().create_stream(
///     MessageCreateBuilder::new("claude-3-5-sonnet-latest", 1024)
///         .user("Write a story about AI")
///         .stream(true)
///         .build()
/// ).await?;
///
/// let final_message = stream
///     .on_text(|delta, _snapshot| {
///         print!("{}", delta);
///     })
///     .on_error(|error| {
///         eprintln!("Stream error: {}", error);
///     })
///     .final_message().await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Async iteration:
/// ```ignore
/// # use agentik_sdk::{Anthropic, MessageCreateBuilder, MessageStreamEvent};
/// # use futures::StreamExt;
/// # async fn example() -> agentik_sdk::Result<()> {
/// let client = Anthropic::new("your-api-key")?;
/// let mut stream = client.messages().create_stream(
///     MessageCreateBuilder::new("claude-3-5-sonnet-latest", 1024)
///         .user("Tell me a joke")
///         .stream(true)
///         .build()
/// ).await?;
///
/// while let Some(event) = stream.next().await {
///     match event? {
///         MessageStreamEvent::ContentBlockDelta { delta, .. } => {
///             // Process incremental content
///         }
///         MessageStreamEvent::MessageStop => break,
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[pin_project]
pub struct MessageStream {
    /// Current accumulated message snapshot
    current_message: Arc<Mutex<Option<Message>>>,
    
    /// Event handlers for different event types
    event_handlers: Arc<Mutex<HashMap<EventType, Vec<EventHandler>>>>,
    
    /// Broadcast channel for distributing events to handlers
    event_sender: broadcast::Sender<MessageStreamEvent>,
    
    /// Stream for events from the underlying HTTP stream
    #[pin]
    event_stream: BroadcastStream<MessageStreamEvent>,
    
    /// Channel for signaling when the stream ends
    completion_sender: Option<oneshot::Sender<Result<Message>>>,
    completion_receiver: oneshot::Receiver<Result<Message>>,
    
    /// Whether the stream has ended
    ended: Arc<Mutex<bool>>,
    
    /// Whether an error occurred
    errored: Arc<Mutex<bool>>,
    
    /// Whether the stream was aborted by the user
    aborted: Arc<Mutex<bool>>,
    
    /// Response metadata
    response: Option<reqwest::Response>,
    request_id: Option<String>,

    /// Handle to the background stream-processing task.
    /// Shared via Arc so abort() can cancel it from &self.
    _background_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl MessageStream {
    /// Create a new MessageStream from an HTTP response.
    ///
    /// This is typically called internally by the SDK when creating streaming requests.
    pub fn new(response: reqwest::Response, request_id: Option<String>) -> Self {
        let (event_sender, event_receiver) = broadcast::channel(1000);
        let (completion_sender, completion_receiver) = oneshot::channel();
        
        Self {
            current_message: Arc::new(Mutex::new(None)),
            event_handlers: Arc::new(Mutex::new(HashMap::new())),
            event_sender,
            event_stream: BroadcastStream::new(event_receiver),
            completion_sender: Some(completion_sender),
            completion_receiver,
            ended: Arc::new(Mutex::new(false)),
            errored: Arc::new(Mutex::new(false)),
            aborted: Arc::new(Mutex::new(false)),
            response: Some(response),
            request_id,
            _background_task: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a MessageStream from a predefined list of events and a final message.
    ///
    /// Intended for unit tests. A background task drains the event list into the
    /// broadcast channel, then delivers `final_message` through the oneshot
    /// completion channel (just like `from_http_stream` does).
    #[cfg(test)]
    pub fn from_events(
        events: Vec<MessageStreamEvent>,
        final_message: Message,
    ) -> Self {
        let (event_sender, event_receiver) = broadcast::channel(events.len().max(1));
        let (completion_sender, completion_receiver) = oneshot::channel();

        let current_message = Arc::new(Mutex::new(None));
        let ended = Arc::new(Mutex::new(false));
        let errored = Arc::new(Mutex::new(false));

        let cm = current_message.clone();
        let end = ended.clone();
        let tx = event_sender.clone();

        tokio::spawn(async move {
            for event in &events {
                match event {
                    MessageStreamEvent::MessageStart { message } => {
                        *cm.lock().unwrap() = Some(message.clone());
                    }
                    MessageStreamEvent::ContentBlockStart { content_block, index } => {
                        if let Some(msg) = cm.lock().unwrap().as_mut() {
                            while msg.content.len() <= *index {
                                msg.content.push(ContentBlock::Text {
                                    text: String::new(),
                                });
                            }
                            msg.content[*index] = content_block.clone();
                        }
                    }
                    MessageStreamEvent::ContentBlockDelta { delta, index } => {
                        if let Some(msg) = cm.lock().unwrap().as_mut() {
                            if let Some(block) = msg.content.get_mut(*index) {
                                match (block, delta) {
                                    (ContentBlock::Text { text }, ContentBlockDelta::TextDelta { text: delta_text }) => {
                                        text.push_str(delta_text);
                                    }
                                    (ContentBlock::Thinking { thinking, .. }, ContentBlockDelta::ThinkingDelta { thinking: delta_thinking }) => {
                                        thinking.push_str(delta_thinking);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    MessageStreamEvent::ContentBlockStop { .. } => {}
                    MessageStreamEvent::MessageDelta { delta, usage } => {
                        if let Some(msg) = cm.lock().unwrap().as_mut() {
                            msg.stop_reason = delta.stop_reason.clone();
                            if let Some(msg_usage) = msg.usage.as_mut() {
                                msg_usage.output_tokens = usage.output_tokens;
                                if let Some(in_tokens) = usage.input_tokens {
                                    msg_usage.input_tokens = in_tokens;
                                }
                            }
                        }
                    }
                    MessageStreamEvent::MessageStop => {}
                }
                let _ = tx.send(event.clone());
            }
            let _ = completion_sender.send(Ok(final_message));
            *end.lock().unwrap() = true;
        });

        Self {
            current_message,
            event_handlers: Arc::new(Mutex::new(HashMap::new())),
            event_sender,
            event_stream: BroadcastStream::new(event_receiver),
            completion_sender: None,
            completion_receiver,
            ended,
            errored,
            aborted: Arc::new(Mutex::new(false)),
            response: None,
            request_id: None,
            _background_task: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new MessageStream from an HttpStreamClient.
    ///
    /// This connects a real HTTP stream to the MessageStream, providing
    /// proper streaming functionality for real-time response processing.
    ///
    /// This entry point is retry-free: if the underlying connection
    /// stalls, the stream terminates with an error. Use
    /// [`from_http_stream_with_retry`] (or
    /// [`crate::resources::MessagesResource::create_stream_with_config`])
    /// to get automatic reconnect on pre-`MessageStart` stalls.
    pub fn from_http_stream(http_stream: crate::http::streaming::HttpStreamClient) -> Result<Self> {
        // Disable retries at the SDK layer by handing in a reconnect
        // closure that always fails with a clear error. The single
        // attempt that just produced `http_stream` is then consumed by
        // the background task as usual.
        let never_reconnect = || async {
            Err(crate::types::AnthropicError::StreamError(
                "retry disabled: stream was not configured with from_http_stream_with_retry"
                    .to_string(),
            ))
        };
        let config = http_stream.config().clone();
        Self::from_http_stream_with_retry(http_stream, never_reconnect, config)
    }

    /// Create a MessageStream that transparently reconnects on stalls
    /// **before** any `MessageStart` event has been observed.
    ///
    /// Semantics:
    /// - Each chunk is gated by `config.event_timeout` seconds. If no
    ///   event arrives in that window, the current HttpStreamClient is
    ///   dropped (canceling its underlying reqwest connection) and
    ///   `reconnect` is invoked to open a fresh one. The retry counter
    ///   is bounded by `config.max_retries`.
    /// - Once a `MessageStart` event has been emitted, the stream is
    ///   considered "in flight" and any subsequent timeout / network
    ///   error terminates the stream with
    ///   [`crate::types::AnthropicError::StreamError`] (the partial
    ///   message accumulated so far is still returned via
    ///   `final_message()`).
    /// - If `reconnect` itself returns an error, the retry counter is
    ///   still consumed and the stream terminates with that error
    ///   once `max_retries` is exhausted.
    /// - Setting `config.retry_on_error = false` short-circuits retries
    ///   entirely: a stall immediately terminates the stream.
    pub fn from_http_stream_with_retry<F, Fut>(
        mut http_stream: crate::http::streaming::HttpStreamClient,
        reconnect: F,
        config: crate::http::streaming::StreamConfig,
    ) -> Result<Self>
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<crate::http::streaming::HttpStreamClient>>
            + Send
            + 'static,
    {
        let (event_sender, event_receiver) = broadcast::channel(config.buffer_size);
        let (completion_sender, completion_receiver) = oneshot::channel();

        let current_message = Arc::new(Mutex::new(None));
        let ended = Arc::new(Mutex::new(false));
        let errored = Arc::new(Mutex::new(false));
        let aborted = Arc::new(Mutex::new(false));
        let request_id = http_stream.request_id().map(|s| s.to_string());

        // Idle timeout (per chunk). Default 30s, matching StreamConfig.
        let event_timeout_secs = config.event_timeout.unwrap_or(30);
        let max_retries = config.max_retries.unwrap_or(3);
        let retry_on_error = config.retry_on_error;

        // Clones handed off to the background task.
        let current_message_clone = current_message.clone();
        let ended_clone = ended.clone();
        let errored_clone = errored.clone();
        let aborted_clone = aborted.clone();
        let event_sender_clone = event_sender.clone();

        let bg_handle = Arc::new(Mutex::new(None));
        let bg_handle_clone = bg_handle.clone();

        let background_handle = tokio::spawn(async move {
            use futures::StreamExt;

            let mut final_message: Option<crate::types::Message> = None;
            let mut completion_sender = Some(completion_sender);
            let timeout_duration = std::time::Duration::from_secs(event_timeout_secs);
            let mut saw_message_start = false;
            let mut attempts: u32 = 0;
            // We treat the initial HttpStreamClient as the first
            // attempt. Reconnects (if any) start counting from 0 again.
            let mut retries_used: u32 = 0;

            'outer: loop {
                if *aborted_clone.lock().unwrap() {
                    break;
                }

                loop {
                    let next_result =
                        tokio::time::timeout(timeout_duration, http_stream.next()).await;

                    match next_result {
                        Err(_elapsed) => {
                            // Idle timeout — no event received within threshold.
                            tracing::warn!(
                                timeout_secs = event_timeout_secs,
                                attempt = attempts,
                                saw_message_start,
                                "stream idle timeout: no event received"
                            );
                            if !saw_message_start && retry_on_error
                                && retries_used < max_retries
                            {
                                retries_used += 1;
                                attempts = 0;
                                tracing::info!(
                                    retry = retries_used,
                                    max_retries,
                                    "reconnecting stream after idle timeout (no MessageStart yet)"
                                );
                                match reconnect().await {
                                    Ok(new_stream) => {
                                        // Drop the old HttpStreamClient —
                                        // this cancels the underlying
                                        // reqwest Response because the
                                        // bytes_stream() future is no
                                        // longer polled.
                                        http_stream = new_stream;
                                        // Restart the inner loop with a
                                        // fresh stream.
                                        continue 'outer;
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            error = %e,
                                            "reconnect attempt failed"
                                        );
                                        *errored_clone.lock().unwrap() = true;
                                        if let Some(sender) = completion_sender.take() {
                                            let _ = sender.send(Err(e));
                                        }
                                        break 'outer;
                                    }
                                }
                            }
                            *errored_clone.lock().unwrap() = true;
                            if let Some(sender) = completion_sender.take() {
                                let _ = sender.send(Err(
                                    if saw_message_start {
                                        crate::types::AnthropicError::StreamError(format!(
                                            "stream stalled after MessageStart \
                                             (no event for {event_timeout_secs}s)"
                                        ))
                                    } else {
                                        crate::types::AnthropicError::Timeout
                                    },
                                ));
                            }
                            break 'outer;
                        }
                        Ok(Some(event_result)) => {
                            match event_result {
                                Ok(event) => {
                                    if matches!(
                                        event,
                                        crate::types::MessageStreamEvent::MessageStart { .. }
                                    ) {
                                        saw_message_start = true;
                                    }

                                    // Update current message state
                                    match &event {
                                        crate::types::MessageStreamEvent::MessageStart { message } => {
                                            *current_message_clone.lock().unwrap() = Some(message.clone());
                                            final_message = Some(message.clone());
                                        }
                                        crate::types::MessageStreamEvent::ContentBlockStart { content_block, index } => {
                                            if let Some(ref mut msg) = *current_message_clone.lock().unwrap() {
                                                while msg.content.len() <= *index {
                                                    msg.content.push(crate::types::ContentBlock::Text { text: String::new() });
                                                }
                                                msg.content[*index] = content_block.clone();
                                            }
                                            if let Some(ref mut msg) = final_message.as_mut() {
                                                while msg.content.len() <= *index {
                                                    msg.content.push(crate::types::ContentBlock::Text { text: String::new() });
                                                }
                                                msg.content[*index] = content_block.clone();
                                            }
                                        }
                                        crate::types::MessageStreamEvent::ContentBlockDelta { delta, index } => {
                                            if let Some(ref mut msg) = *current_message_clone.lock().unwrap()
                                                && let Some(content_block) = msg.content.get_mut(*index)
                                            {
                                                Self::apply_delta_inline(content_block, delta);
                                            }
                                            if let Some(ref mut msg) = final_message.as_mut()
                                                && let Some(content_block) = msg.content.get_mut(*index)
                                            {
                                                Self::apply_delta_inline(content_block, delta);
                                            }
                                        }
                                        crate::types::MessageStreamEvent::MessageDelta { delta, usage } => {
                                            let update_usage = |msg: &mut crate::types::Message| {
                                                if let Some(stop_reason) = &delta.stop_reason {
                                                    msg.stop_reason = Some(stop_reason.clone());
                                                }
                                                if let Some(stop_sequence) = &delta.stop_sequence {
                                                    msg.stop_sequence = Some(stop_sequence.clone());
                                                }
                                                let u = msg.usage.get_or_insert_with(crate::types::Usage::default);
                                                u.output_tokens = usage.output_tokens;
                                                if let Some(input_tokens) = usage.input_tokens {
                                                    u.input_tokens = input_tokens;
                                                }
                                                if let Some(cache_creation) = usage.cache_creation_input_tokens {
                                                    u.cache_creation_input_tokens = Some(cache_creation);
                                                }
                                                if let Some(cache_read) = usage.cache_read_input_tokens {
                                                    u.cache_read_input_tokens = Some(cache_read);
                                                }
                                            };
                                            if let Some(ref mut msg) = *current_message_clone.lock().unwrap() {
                                                update_usage(msg);
                                            }
                                            if let Some(ref mut msg) = final_message.as_mut() {
                                                update_usage(msg);
                                            }
                                        }
                                        crate::types::MessageStreamEvent::MessageStop => {
                                            *ended_clone.lock().unwrap() = true;
                                            if let Some(sender) = completion_sender.take() {
                                                if let Some(message) = final_message.clone() {
                                                    let _ = sender.send(Ok(message));
                                                } else {
                                                    let _ = sender.send(Err(crate::types::AnthropicError::StreamError(
                                                        "Stream ended without message".to_string()
                                                    )));
                                                }
                                            }
                                            if let Err(e) = event_sender_clone.send(event) {
                                                tracing::debug!("broadcast send failed (receiver lagged): {e}");
                                            }
                                            break 'outer;
                                        }
                                        _ => {}
                                    }

                                    if let Err(e) = event_sender_clone.send(event) {
                                        tracing::debug!("broadcast send failed (receiver lagged): {e}");
                                    }
                                }
                                Err(e) => {
                                    let is_timeout = matches!(e, crate::types::AnthropicError::Timeout);
                                    let stallable = matches!(
                                        e,
                                        crate::types::AnthropicError::NetworkError(_)
                                            | crate::types::AnthropicError::Connection { .. }
                                            | crate::types::AnthropicError::StreamError(_)
                                    ) || is_timeout;

                                    if !saw_message_start
                                        && retry_on_error
                                        && stallable
                                        && retries_used < max_retries
                                    {
                                        retries_used += 1;
                                        attempts = 0;
                                        tracing::warn!(
                                            error = %e,
                                            retry = retries_used,
                                            max_retries,
                                            "reconnecting stream after error (no MessageStart yet)"
                                        );
                                        match reconnect().await {
                                            Ok(new_stream) => {
                                                http_stream = new_stream;
                                                continue 'outer;
                                            }
                                            Err(reconnect_err) => {
                                                tracing::error!(
                                                    error = %reconnect_err,
                                                    "reconnect attempt failed"
                                                );
                                                *errored_clone.lock().unwrap() = true;
                                                if let Some(sender) = completion_sender.take() {
                                                    let _ = sender.send(Err(reconnect_err));
                                                }
                                                break 'outer;
                                            }
                                        }
                                    }
                                    *errored_clone.lock().unwrap() = true;
                                    if let Some(sender) = completion_sender.take() {
                                        let _ = sender.send(Err(e));
                                    }
                                    break 'outer;
                                }
                            }
                        }
                        Ok(None) => {
                            // Stream naturally ended
                            break 'outer;
                        }
                    }
                }
            }
            // Fallback: if the stream ended without a MessageStop event,
            // still deliver whatever message was accumulated so that
            // final_message() does not hang forever.
            if let Some(sender) = completion_sender.take() {
                let _ = sender.send(if let Some(msg) = final_message {
                    Ok(msg)
                } else {
                    Err(crate::types::AnthropicError::StreamError(
                        "Stream ended without message".to_string(),
                    ))
                });
            }
            *ended_clone.lock().unwrap() = true;
        });
        *bg_handle_clone.lock().unwrap() = Some(background_handle);

        Ok(Self {
            current_message,
            event_handlers: Arc::new(Mutex::new(HashMap::new())),
            event_sender,
            event_stream: BroadcastStream::new(event_receiver),
            completion_sender: None, // Already consumed by the task
            completion_receiver,
            ended,
            errored,
            aborted,
            response: None, // No response needed for HTTP stream
            request_id,
            _background_task: bg_handle,
        })
    }
    
    /// Register a callback for text delta events.
    ///
    /// The callback receives two parameters:
    /// - `delta`: The new text being appended
    /// - `snapshot`: The current accumulated text
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::MessageStream;
    /// # async fn example(stream: MessageStream) {
    /// stream.on_text(|delta, snapshot| {
    ///     print!("{}", delta);
    ///     println!("Total so far: {}", snapshot);
    /// });
    /// # }
    /// ```
    pub fn on_text<F>(self, callback: F) -> Self
    where
        F: Fn(&str, &str) + Send + Sync + 'static,
    {
        self.on(EventType::Text, EventHandler::Text(Box::new(callback)))
    }
    
    /// Register a callback for stream events.
    ///
    /// This provides access to all raw stream events and the current message snapshot.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::{MessageStream, MessageStreamEvent, Message};
    /// # async fn example(stream: MessageStream) {
    /// stream.on_stream_event(|event, snapshot| {
    ///     match event {
    ///         MessageStreamEvent::ContentBlockStart { .. } => {
    ///             println!("New content block started");
    ///         }
    ///         _ => {}
    ///     }
    /// });
    /// # }
    /// ```
    pub fn on_stream_event<F>(self, callback: F) -> Self
    where
        F: Fn(&MessageStreamEvent, &Message) + Send + Sync + 'static,
    {
        self.on(EventType::StreamEvent, EventHandler::StreamEvent(Box::new(callback)))
    }
    
    /// Register a callback for when a complete message is received.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::{MessageStream, Message};
    /// # async fn example(stream: MessageStream) {
    /// stream.on_message(|message| {
    ///     println!("Received message: {:?}", message);
    /// });
    /// # }
    /// ```
    pub fn on_message<F>(self, callback: F) -> Self
    where
        F: Fn(&Message) + Send + Sync + 'static,
    {
        self.on(EventType::Message, EventHandler::Message(Box::new(callback)))
    }
    
    /// Register a callback for when the final message is complete.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::{MessageStream, Message};
    /// # async fn example(stream: MessageStream) {
    /// stream.on_final_message(|message| {
    ///     println!("Final message: {:?}", message);
    /// });
    /// # }
    /// ```
    pub fn on_final_message<F>(self, callback: F) -> Self
    where
        F: Fn(&Message) + Send + Sync + 'static,
    {
        self.on(EventType::FinalMessage, EventHandler::FinalMessage(Box::new(callback)))
    }
    
    /// Register a callback for errors.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::{MessageStream, AnthropicError};
    /// # async fn example(stream: MessageStream) {
    /// stream.on_error(|error| {
    ///     eprintln!("Stream error: {}", error);
    /// });
    /// # }
    /// ```
    pub fn on_error<F>(self, callback: F) -> Self
    where
        F: Fn(&AnthropicError) + Send + Sync + 'static,
    {
        self.on(EventType::Error, EventHandler::Error(Box::new(callback)))
    }
    
    /// Register a callback for when the stream ends.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::MessageStream;
    /// # async fn example(stream: MessageStream) {
    /// stream.on_end(|| {
    ///     println!("Stream ended");
    /// });
    /// # }
    /// ```
    pub fn on_end<F>(self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on(EventType::End, EventHandler::End(Box::new(callback)))
    }
    
    /// Generic method to register event handlers.
    fn on(self, event_type: EventType, handler: EventHandler) -> Self {
        {
            let mut handlers = self.event_handlers.lock().unwrap();
            handlers.entry(event_type).or_default().push(handler);
        }
        self
    }
    
    /// Wait for the stream to complete and return the final message.
    ///
    /// This method will block until the stream ends and return the accumulated message.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::MessageStream;
    /// # async fn example(stream: MessageStream) -> agentik_sdk::Result<()> {
    /// let final_message = stream.final_message().await?;
    /// println!("Claude said: {:?}", final_message.content);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn final_message(self) -> Result<Message> {
        let Self { completion_receiver, .. } = self;
        completion_receiver.await
            .map_err(|_| AnthropicError::StreamError("Stream ended unexpectedly".to_string()))?
    }
    
    /// Wait for the stream to complete without returning the message.
    ///
    /// This is useful when you're processing events with callbacks and just need
    /// to wait for completion.
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use agentik_sdk::MessageStream;
    /// # async fn example(stream: MessageStream) -> agentik_sdk::Result<()> {
    /// stream.on_text(|delta, _| print!("{}", delta))
    ///     .done().await?;
    /// println!("\nStream completed!");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn done(self) -> Result<()> {
        let Self { completion_receiver, .. } = self;
        completion_receiver.await
            .map_err(|_| AnthropicError::StreamError("Stream ended unexpectedly".to_string()))?
            .map(|_| ())
    }
    
    /// Get the current accumulated message snapshot.
    ///
    /// Returns `None` if the stream hasn't started or no message has been received yet.
    pub fn current_message(&self) -> Option<Message> {
        self.current_message.lock().unwrap().clone()
    }
    
    /// Check if the stream has ended.
    pub fn ended(&self) -> bool {
        *self.ended.lock().unwrap()
    }
    
    /// Check if an error occurred.
    pub fn errored(&self) -> bool {
        *self.errored.lock().unwrap()
    }
    
    /// Check if the stream was aborted.
    pub fn aborted(&self) -> bool {
        *self.aborted.lock().unwrap()
    }
    
    /// Get the response metadata.
    pub fn response(&self) -> Option<&reqwest::Response> {
        self.response.as_ref()
    }
    
    /// Get the request ID.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
    
    /// Abort the stream.
    ///
    /// Marks the stream as aborted and cancels the background task.
    pub fn abort(&self) {
        *self.aborted.lock().unwrap() = true;
        if let Some(handle) = self._background_task.lock().unwrap().take() {
            handle.abort();
        }
    }

    /// Process a stream event and update the internal state.
    ///
    /// This method accumulates message content from incremental updates and
    /// dispatches events to registered handlers.
    #[allow(dead_code)]
    fn process_event(&self, event: MessageStreamEvent) -> Result<()> {
        // Update current message state based on the event
        match &event {
            MessageStreamEvent::MessageStart { message } => {
                *self.current_message.lock().unwrap() = Some(message.clone());
            }
            MessageStreamEvent::ContentBlockStart { content_block, index } => {
                if let Some(ref mut msg) = *self.current_message.lock().unwrap() {
                    // Ensure the content array is large enough
                    while msg.content.len() <= *index {
                        msg.content.push(ContentBlock::Text { text: String::new() });
                    }
                    msg.content[*index] = content_block.clone();
                }
            }
            MessageStreamEvent::ContentBlockDelta { delta, index } => {
                if let Some(ref mut msg) = *self.current_message.lock().unwrap()
                    && let Some(content_block) = msg.content.get_mut(*index)
                {
                    self.apply_delta(content_block, delta)?;
                }
            }
            MessageStreamEvent::MessageDelta { delta, usage } => {
                if let Some(ref mut msg) = *self.current_message.lock().unwrap() {
                    if let Some(stop_reason) = &delta.stop_reason {
                        msg.stop_reason = Some(stop_reason.clone());
                    }
                    if let Some(stop_sequence) = &delta.stop_sequence {
                        msg.stop_sequence = Some(stop_sequence.clone());
                    }
                    let u = msg.usage.get_or_insert_with(crate::types::Usage::default);
                    u.output_tokens = usage.output_tokens;
                    if let Some(input_tokens) = usage.input_tokens {
                        u.input_tokens = input_tokens;
                    }
                    if let Some(cache_creation) = usage.cache_creation_input_tokens {
                        u.cache_creation_input_tokens = Some(cache_creation);
                    }
                    if let Some(cache_read) = usage.cache_read_input_tokens {
                        u.cache_read_input_tokens = Some(cache_read);
                    }
                }
            }
            MessageStreamEvent::MessageStop => {
                *self.ended.lock().unwrap() = true;
            }
            _ => {}
        }
        
        // Dispatch event to handlers
        self.dispatch_event(&event)?;
        
        // Send event to broadcast channel for async iteration
        let _ = self.event_sender.send(event);
        
        Ok(())
    }
    
    /// Apply a content block delta to update the content.
    #[allow(dead_code)]
    fn apply_delta(&self, content_block: &mut ContentBlock, delta: &ContentBlockDelta) -> Result<()> {
        match (content_block, delta) {
            (ContentBlock::Text { text }, ContentBlockDelta::TextDelta { text: delta_text }) => {
                text.push_str(delta_text);
            }
            (ContentBlock::Thinking { thinking, .. }, ContentBlockDelta::ThinkingDelta { thinking: delta_thinking }) => {
                thinking.push_str(delta_thinking);
            }
            (ContentBlock::Thinking { signature, .. }, ContentBlockDelta::SignatureDelta { signature: delta_sig }) => {
                signature.push_str(delta_sig);
            }
            (ContentBlock::ToolUse { input, .. }, ContentBlockDelta::InputJsonDelta { partial_json }) => {
                // In a real implementation, we'd parse the partial JSON
                // For now, we'll just store it as-is
                *input = serde_json::from_str(partial_json)
                    .unwrap_or_else(|_| serde_json::Value::String(partial_json.clone()));
            }
            _ => {}
        }
        Ok(())
    }

    /// Inline version of apply_delta for use in from_http_stream (no &self needed).
    fn apply_delta_inline(content_block: &mut ContentBlock, delta: &ContentBlockDelta) {
        match (content_block, delta) {
            (ContentBlock::Text { text }, ContentBlockDelta::TextDelta { text: delta_text }) => {
                text.push_str(delta_text);
            }
            (ContentBlock::Thinking { thinking, .. }, ContentBlockDelta::ThinkingDelta { thinking: delta_thinking }) => {
                thinking.push_str(delta_thinking);
            }
            (ContentBlock::Thinking { signature, .. }, ContentBlockDelta::SignatureDelta { signature: delta_sig }) => {
                signature.push_str(delta_sig);
            }
            (ContentBlock::ToolUse { input, .. }, ContentBlockDelta::InputJsonDelta { partial_json }) => {
                *input = serde_json::from_str(partial_json)
                    .unwrap_or_else(|_| serde_json::Value::String(partial_json.clone()));
            }
            _ => {}
        }
    }

    /// Dispatch an event to all registered handlers.
    fn dispatch_event(&self, event: &MessageStreamEvent) -> Result<()> {
        let handlers = self.event_handlers.lock().unwrap();
        let current_message = self.current_message.lock().unwrap();
        
        // Dispatch to stream event handlers
        if let Some(stream_handlers) = handlers.get(&EventType::StreamEvent) {
            for handler in stream_handlers {
                if let EventHandler::StreamEvent(callback) = handler
                    && let Some(ref msg) = *current_message
                {
                    callback(event, msg);
                }
            }
        }
        
        // Dispatch specific event types
        match event {
            MessageStreamEvent::ContentBlockDelta { delta, .. } => {
                if let ContentBlockDelta::TextDelta { text } = delta
                    && let Some(text_handlers) = handlers.get(&EventType::Text)
                {
                    for handler in text_handlers {
                        if let EventHandler::Text(callback) = handler {
                            // Get current accumulated text for snapshot
                            let snapshot = if let Some(ref msg) = *current_message {
                                self.get_accumulated_text(msg)
                            } else {
                                String::new()
                            };
                            callback(text, &snapshot);
                        }
                    }
                }
            }
            MessageStreamEvent::MessageStop => {
                if let Some(end_handlers) = handlers.get(&EventType::End) {
                    for handler in end_handlers {
                        if let EventHandler::End(callback) = handler {
                            callback();
                        }
                    }
                }
                
                // Send final message
                if let Some(ref msg) = *current_message
                    && let Some(final_handlers) = handlers.get(&EventType::FinalMessage)
                {
                    for handler in final_handlers {
                        if let EventHandler::FinalMessage(callback) = handler {
                            callback(msg);
                        }
                    }
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// Get the accumulated text from all text content blocks.
    fn get_accumulated_text(&self, message: &Message) -> String {
        message.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

impl Stream for MessageStream {
    type Item = Result<MessageStreamEvent>;
    
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use futures::Stream as FuturesStream;

        let this = self.project();

        // When the background SSE-processing task has finished (ended == true)
        // and all buffered broadcast events have been drained, signal
        // stream termination.  Without this check the BroadcastStream
        // would return Pending forever because the `event_sender` stored
        // in this struct keeps the broadcast channel open.
        if *this.ended.lock().unwrap() {
            return match FuturesStream::poll_next(this.event_stream, cx) {
                std::task::Poll::Ready(Some(Ok(event))) => {
                    std::task::Poll::Ready(Some(Ok(event)))
                }
                _ => std::task::Poll::Ready(None),
            };
        }

        match FuturesStream::poll_next(this.event_stream, cx) {
            std::task::Poll::Ready(Some(Ok(event))) => {
                std::task::Poll::Ready(Some(Ok(event)))
            }
            std::task::Poll::Ready(Some(Err(err))) => {
                tracing::warn!("broadcast channel lagged: {err}");
                std::task::Poll::Ready(Some(Err(AnthropicError::StreamError(
                    format!("Stream lagged: {}", err)
                ))))
            }
            std::task::Poll::Ready(None) => {
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Role, Usage};
    
    // For testing, we'll use a simple helper to create a dummy response
    async fn create_dummy_response() -> reqwest::Response {
        // Create a simple HTTP client and make a basic request for testing
        let client = reqwest::Client::new();
        // Use httpbin.org which provides testing endpoints
        client.get("https://httpbin.org/status/200")
            .send()
            .await
            .expect("Failed to create test response")
    }
    
    #[tokio::test]
    async fn test_message_stream_creation() {
        let response = create_dummy_response().await;
        let stream = MessageStream::new(response, Some("test-request-id".to_string()));
        
        assert!(!stream.ended());
        assert!(!stream.errored());
        assert!(!stream.aborted());
        assert_eq!(stream.request_id(), Some("test-request-id"));
    }
    
    #[tokio::test]
    async fn test_event_processing() {
        let response = create_dummy_response().await;
        let stream = MessageStream::new(response, None);
        
        // Test message start event
        let start_event = MessageStreamEvent::MessageStart {
            message: Message {
                id: "msg_test".to_string(),
                type_: "message".to_string(),
                role: Role::Assistant,
                content: vec![],
                model: Some("claude-3-5-sonnet-latest".to_string()),
                stop_reason: None,
                stop_sequence: None,
                usage: Some(Usage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                    server_tool_use: None,
                    service_tier: None,
                }),
                request_id: None,
            },
        };
        
        stream.process_event(start_event).unwrap();
        
        let current = stream.current_message().unwrap();
        assert_eq!(current.id, "msg_test");
        assert_eq!(current.role, Role::Assistant);
    }
    
    #[test]
    fn test_event_handlers() {
        use std::sync::{Arc, Mutex};
        use std::collections::HashMap;
        
        // Test creating a dummy response for testing
        let text_called = Arc::new(Mutex::new(false));
        let text_called_clone = text_called.clone();
        
        let _handler = EventHandler::Text(Box::new(move |_delta, _snapshot| {
            *text_called_clone.lock().unwrap() = true;
        }));
        
        // Test event type equality
        assert_eq!(EventType::Text, EventType::Text);
        assert_ne!(EventType::Text, EventType::Error);
        
        // Test using event types as hash keys
        let mut map: HashMap<EventType, String> = HashMap::new();
        map.insert(EventType::Text, "text_handler".to_string());
        assert_eq!(map.get(&EventType::Text), Some(&"text_handler".to_string()));
    }

    /// Spin up a single-shot HTTP/1.1 server that responds with the
    /// provided raw bytes and then closes the connection. Used to
    /// exercise the SSE stream end-to-end without hitting the network.
    async fn single_shot_server(
        reply: &'static [u8],
        delay_before_reply: std::time::Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                // Read the request (we don't actually care about its content)
                let mut buf = vec![0u8; 4096];
                let _ = sock.read(&mut buf).await;
                if !delay_before_reply.is_zero() {
                    tokio::time::sleep(delay_before_reply).await;
                }
                // Wrap the raw SSE body in a minimal valid HTTP/1.1
                // response so reqwest/hyper can parse it. We avoid
                // declaring Content-Length or Transfer-Encoding; a
                // connection close marks the end of the body, which is
                // the path we want to exercise (eventsource-stream
                // converts the close into a clean `None`).
                let response = format!(
                    "HTTP/1.1 200 OK\r\n\
                     content-type: text/event-stream\r\n\
                     connection: close\r\n\
                     \r\n"
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.write_all(reply).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://{addr}"), handle)
    }

    /// Server variant: accepts the connection, sends valid HTTP
    /// response headers, then deliberately stalls forever (does not
    /// write a body, does not close). Used to drive the client's
    /// per-chunk idle timeout.
    async fn single_shot_stall_server() -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let headers = format!(
                    "HTTP/1.1 200 OK\r\n\
                     content-type: text/event-stream\r\n\
                     connection: close\r\n\
                     \r\n"
                );
                let _ = sock.write_all(headers.as_bytes()).await;
                // Sleep "forever" from the test's perspective — the
                // test's tokio clock is paused, so this would normally
                // block forever. We instead just hold the socket open.
                // The test will exit and the handle is dropped, which
                // closes the socket.
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                }
            }
        });
        (format!("http://{addr}"), handle)
    }

    /// Valid SSE body mirroring a complete (but minimal) Anthropic
    /// streaming response.
    const VALID_SSE: &[u8] = b"\
event: message_start\r\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_x\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"cache_creation_input_tokens\":null,\"cache_read_input_tokens\":null,\"server_tool_use\":null,\"service_tier\":null}}}\r\n\
\r\n\
event: content_block_start\r\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\r\n\
\r\n\
event: content_block_delta\r\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\r\n\
\r\n\
event: content_block_stop\r\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\r\n\
\r\n\
event: message_delta\r\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":2,\"input_tokens\":1,\"cache_creation_input_tokens\":null,\"cache_read_input_tokens\":null}}\r\n\
\r\n\
event: message_stop\r\n\
data: {\"type\":\"message_stop\"}\r\n\
\r\n";

    #[tokio::test(start_paused = true)]
    async fn from_http_stream_with_retry_idle_timeout_then_reconnect_succeeds() {
        // First server accepts the connection, sends the HTTP response
        // headers, then never writes any body and never closes — the
        // client's per-chunk idle timeout (1s) should fire and trigger
        // a reconnect. The second server returns a valid SSE body.
        let (url1, _h1) = single_shot_stall_server().await;
        let (url2, h2) = single_shot_server(VALID_SSE, std::time::Duration::ZERO).await;
        let _keep_h2 = h2; // ensure it lives long enough

        let client = reqwest::Client::new();
        let first_response = client
            .get(&url1)
            .send()
            .await
            .expect("first response should be reachable");

        let config = crate::http::streaming::StreamConfig {
            buffer_size: 64,
            event_timeout: Some(1),
            retry_on_error: true,
            max_retries: Some(2),
        };

        // Build a real HttpStreamClient from the first (stalled) response.
        let http_stream =
            crate::http::streaming::HttpStreamClient::from_response(first_response, config.clone())
                .await
                .expect("from_response should succeed for 200");

        // Reconnect closure: re-issue the same GET against a *different*
        // URL (the second server). In real code it would re-issue the
        // original POST; for this test we just want to prove the
        // background task will call it and resume on success.
        let url2 = url2.clone();
        let reconnect_config = config.clone();
        let reconnect = move || {
            let url2 = url2.clone();
            let client = client.clone();
            let config = reconnect_config.clone();
            async move {
                let resp = client.get(&url2).send().await.map_err(|e| {
                    crate::types::AnthropicError::Connection { message: e.to_string() }
                })?;
                crate::http::streaming::HttpStreamClient::from_response(resp, config).await
            }
        };

        let stream = MessageStream::from_http_stream_with_retry(http_stream, reconnect, config)
            .expect("from_http_stream_with_retry should construct ok");

        // The test runs under a paused tokio clock. We need to advance
        // it past the 1s idle timeout, then past the second request's
        // round-trip. Spawn a helper that periodically advances the
        // clock so the background task can make progress.
        tokio::spawn(async {
            for _ in 0..50 {
                tokio::time::advance(std::time::Duration::from_millis(200)).await;
            }
        });

        // Bound the wall-clock duration so a regression that drops the
        // timeout still surfaces as a test failure (the paused clock
        // makes this fast).
        let final_msg = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.final_message(),
        )
        .await
        .expect("final_message should resolve in bounded time")
        .expect("final_message should be Ok after a successful reconnect");

        assert_eq!(final_msg.id, "msg_x");
    }

    /// Server variant: accepts the connection, sends valid HTTP
    /// headers, writes a *prefix* body, then stalls forever. Used to
    /// simulate a server that begins the response but freezes
    /// mid-stream.
    async fn single_shot_partial_then_stall_server(
        prefix: &'static [u8],
    ) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let headers = format!(
                    "HTTP/1.1 200 OK\r\n\
                     content-type: text/event-stream\r\n\
                     connection: close\r\n\
                     \r\n"
                );
                let _ = sock.write_all(headers.as_bytes()).await;
                let _ = sock.write_all(prefix).await;
                // Hold the socket open without closing; the test's
                // tokio clock is paused, so this loop is essentially
                // "wait for the test to drop me".
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                }
            }
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test(start_paused = true)]
    async fn from_http_stream_with_retry_post_message_start_does_not_retry() {
        // Server writes MessageStart (and a content_block_start) and
        // then stalls. The retry policy must NOT reconnect here —
        // once we've started receiving the assistant message, partial
        // progress is irrecoverable. The stream should surface the
        // idle-timeout as a StreamError.
        let prefix = b"\
event: message_start\r\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_p\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"cache_creation_input_tokens\":null,\"cache_read_input_tokens\":null,\"server_tool_use\":null,\"service_tier\":null}}}\r\n\
\r\n\
event: content_block_start\r\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\r\n\
\r\n";

        let (url, _h) = single_shot_partial_then_stall_server(prefix).await;

        // Count how many times reconnect is invoked — should be zero.
        let reconnect_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rc = reconnect_calls.clone();
        let reconnect = move || {
            rc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async {
                Err(crate::types::AnthropicError::StreamError(
                    "should not be called after MessageStart".to_string(),
                ))
            }
        };

        let client = reqwest::Client::new();
        let response = client.get(&url).send().await.expect("response");

        let config = crate::http::streaming::StreamConfig {
            buffer_size: 64,
            event_timeout: Some(1),
            retry_on_error: true,
            max_retries: Some(3),
        };

        let http_stream =
            crate::http::streaming::HttpStreamClient::from_response(response, config.clone())
                .await
                .unwrap();

        let stream = MessageStream::from_http_stream_with_retry(http_stream, reconnect, config)
            .expect("construct");

        // Spawn a clock-advance helper so the paused timeout can fire.
        tokio::spawn(async {
            for _ in 0..30 {
                tokio::time::advance(std::time::Duration::from_millis(200)).await;
            }
        });

        // Allow the idle timeout to elapse.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.final_message(),
        )
        .await
        .expect("final_message resolves in bounded time");

        // After MessageStart, the stall must surface as an error rather
        // than trigger a silent retry.
        assert!(
            result.is_err(),
            "expected error after post-start stall, got {:?}",
            result.as_ref().map(|m| m.id.clone())
        );
        assert_eq!(
            reconnect_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "reconnect must not fire after MessageStart"
        );
    }
}