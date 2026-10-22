use crate::{Result, media::Cancellation};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

pub(super) struct Ipc {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    id: u64,
    cancel: Cancellation,
    #[cfg(test)]
    timing: bool,
}
impl Ipc {
    #[cfg(test)]
    pub(super) fn new(stream: UnixStream) -> Result<Self> {
        Self::with_cancel(stream, Cancellation::default())
    }
    pub(super) fn with_cancel(stream: UnixStream, cancel: Cancellation) -> Result<Self> {
        #[cfg(test)]
        let timing = std::env::var("STORYFOLD_PREVIEW_IPC_TIMING").as_deref() == Ok("1");
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_millis(100)))?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            id: 0,
            cancel,
            #[cfg(test)]
            timing,
        })
    }
    pub(super) fn get(&mut self, name: &str) -> Result<Value> {
        self.call(json!(["get_property", name]))
    }
    pub(super) fn call(&mut self, command: Value) -> Result<Value> {
        self.call_with_limit(command, Duration::from_secs(3))
    }
    fn call_with_limit(&mut self, command: Value, limit: Duration) -> Result<Value> {
        let operation = operation(&command);
        let begin = Instant::now();
        self.check_wait(begin, limit, operation)?;
        self.id = self
            .id
            .checked_add(1)
            .ok_or("Preview IPC identity exhausted")?;
        let mut request = serde_json::to_vec(&json!({"command":command,"request_id":self.id}))?;
        request.push(b'\n');
        let mut remaining = request.as_slice();
        // Advance only written bytes. A polling timeout must never replay a
        // command, especially seek/quit or a state-changing native command.
        while !remaining.is_empty() {
            self.check_wait(begin, limit, operation)?;
            self.writer
                .set_write_timeout(Some(wait_interval(begin, limit)))?;
            match self.writer.write(remaining) {
                Ok(0) => {
                    return Err(format!("Native preview {operation} control channel closed").into());
                }
                Ok(count) => remaining = &remaining[count..],
                Err(error) if polling(&error) => continue,
                Err(error) => {
                    return Err(format!(
                        "Native preview {operation} write failed after{}ms: {error}",
                        begin.elapsed().as_millis()
                    )
                    .into());
                }
            }
        }
        let mut line = Vec::new();
        let mut messages = 0;
        loop {
            self.check_wait(begin, limit, operation)?;
            self.reader
                .get_mut()
                .set_read_timeout(Some(wait_interval(begin, limit)))?;
            // Keep partial JSON across receive polls. mpv may finish source/VO
            // initialization between polls; a poll is for cancellation, while
            // the single whole-call deadline decides whether it failed.
            let available = match self.reader.fill_buf() {
                Ok([]) => {
                    return Err(format!("Native preview {operation} control channel closed").into());
                }
                Ok(available) => available,
                Err(error) if polling(&error) => continue,
                Err(error) => {
                    return Err(format!(
                        "Native preview {operation} response failed after{}ms: {error}",
                        begin.elapsed().as_millis()
                    )
                    .into());
                }
            };
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |position| position + 1);
            if line.len() + count > 65_536 {
                return Err("Native preview response exceeded its limit".into());
            }
            line.extend_from_slice(&available[..count]);
            self.reader.consume(count);
            if !line.ends_with(b"\n") {
                continue;
            }
            messages += 1;
            let response: Value = serde_json::from_slice(&line)?;
            line.clear();
            self.check_wait(begin, limit, operation)?;
            if response["request_id"].as_u64() != Some(self.id) {
                if messages >= 256 {
                    return Err("Native preview control channel did not answer".into());
                }
                continue;
            }
            #[cfg(test)]
            if self.timing {
                eprintln!(
                    "STORYFOLD_PREVIEW_IPC_TIMING operation={operation} elapsed_us={}",
                    begin.elapsed().as_micros()
                );
            }
            if response["error"] == "property unavailable" {
                return Ok(Value::Null);
            }
            if response["error"] != "success" {
                return Err(
                    format!("Native preview {operation} command failed; the complete composition was stopped").into(),
                );
            }
            return Ok(response["data"].clone());
        }
    }
    fn check_wait(&self, begin: Instant, limit: Duration, operation: &str) -> Result<()> {
        self.cancel.check()?;
        if begin.elapsed() >= limit {
            return Err(format!(
                "Native preview {operation} response deadline exceeded after{}ms",
                begin.elapsed().as_millis()
            )
            .into());
        }
        Ok(())
    }
}
fn wait_interval(begin: Instant, limit: Duration) -> Duration {
    limit
        .saturating_sub(begin.elapsed())
        .min(Duration::from_millis(100))
        .max(Duration::from_millis(1))
}
fn polling(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}
fn operation(command: &Value) -> &'static str {
    match command
        .get("name")
        .or_else(|| command.get(0))
        .and_then(Value::as_str)
    {
        Some("overlay-add") => "overlay-add",
        Some("overlay-remove") => "overlay-remove",
        Some("get_property") => match command.get(1).and_then(Value::as_str) {
            Some("time-pos") => "get_property time-pos",
            Some("osd-dimensions") => "get_property osd-dimensions",
            _ => "get_property",
        },
        Some("set_property") => "set_property",
        Some("seek") => "seek",
        Some("quit") => "quit",
        Some("screenshot-to-file") => "screenshot-to-file",
        _ => "command",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, thread};

    #[test]
    fn delayed_split_ack_preserves_partial_json_without_resending() {
        let (client, server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let responder = thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["command"], json!(["get_property", "time-pos"]));
            let id = request["request_id"].as_u64().unwrap();
            let mut server = server;
            write!(server, "{{\"request_id\":{id},\"error\":").unwrap();
            thread::sleep(Duration::from_millis(650));
            server.write_all(b"\"success\",\"data\":1.25}\n").unwrap();
            server
                .set_read_timeout(Some(Duration::from_millis(80)))
                .unwrap();
            let extra = reader.read(&mut [0; 1]);
            assert!(
                extra.is_err_and(|error| matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                )),
                "Waiting for an ACK must not resend or duplicate the command"
            );
        });
        let mut ipc = Ipc::new(client).unwrap();
        let result = ipc.get("time-pos");
        responder.join().unwrap();
        assert!(
            result.is_ok(),
            "Late split ACK must retain partial JSON and finish the original request: {result:?}"
        );
        assert_eq!(result.unwrap(), json!(1.25));
    }

    #[test]
    fn cancellation_interrupts_an_outstanding_reply_without_waiting_for_deadline() {
        let (client, _server) = UnixStream::pair().unwrap();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let trigger = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            token.cancel();
        });
        let mut ipc = Ipc::with_cancel(client, cancel).unwrap();
        let begin = Instant::now();
        let error = ipc.get("time-pos").unwrap_err().to_string();
        trigger.join().unwrap();
        assert!(error.contains("cancelled"), "{error}");
        assert!(begin.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn one_deadline_bounds_the_complete_call() {
        let (client, _server) = UnixStream::pair().unwrap();
        let mut ipc = Ipc::new(client).unwrap();
        let begin = Instant::now();
        let error = ipc
            .call_with_limit(
                json!(["get_property", "time-pos"]),
                Duration::from_millis(250),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("response deadline exceeded"), "{error}");
        assert!(begin.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn oversized_or_closed_replies_fail_explicitly() {
        for oversized in [true, false] {
            let (client, server) = UnixStream::pair().unwrap();
            let responder = thread::spawn(move || {
                let mut reader = BufReader::new(server.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                if oversized {
                    let mut server = server;
                    let _ = server.write_all(&vec![b'x'; 65_537]);
                }
            });
            let mut ipc = Ipc::new(client).unwrap();
            let error = ipc.get("time-pos").unwrap_err().to_string();
            responder.join().unwrap();
            assert!(
                error.contains(if oversized {
                    "response exceeded its limit"
                } else {
                    "control channel closed"
                }),
                "{error}"
            );
        }
    }

    #[test]
    fn unrelated_messages_cannot_reset_the_response_budget() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let responder = thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            for _ in 0..256 {
                writeln!(server, "{{\"event\":\"tick\"}}").unwrap();
            }
        });
        let mut ipc = Ipc::new(client).unwrap();
        let error = ipc.get("time-pos").unwrap_err().to_string();
        responder.join().unwrap();
        assert!(error.contains("control channel did not answer"), "{error}");
    }
}
