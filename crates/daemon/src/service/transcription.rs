//! Dictation transcription via the configured Amarcode ACP provider.
//!
//! Audio arrives base64-encoded over RPC, is forwarded in memory to the
//! provider's OpenAI-compatible `/audio/transcriptions` endpoint, and is
//! never written to disk. The provider API key stays daemon-side.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use crate::{protocol::rpc::TranscribeAudioParams, service::AgentManager, Error, Result};

/// Whisper model requested from the provider (OpenRouter-style id).
pub const TRANSCRIPTION_MODEL: &str = "openai/whisper-large-v3-turbo";
/// Provider-side upload limit for transcription audio.
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

pub fn transcribe_audio(agents: &AgentManager, params: TranscribeAudioParams) -> Result<String> {
    let (base_url, api_key) = agents.openai_compatible_provider()?;
    let mime_type = params.mime_type.split(';').next().unwrap_or("").trim();
    let extension = extension_for_mime(mime_type)?;
    if params.audio_base64.trim().is_empty() {
        return Err(Error::msg("No audio was recorded."));
    }
    if params.audio_base64.len() > MAX_AUDIO_BYTES.div_ceil(3) * 4 {
        return Err(Error::msg("Recording exceeds the 25 MB limit."));
    }
    let audio = BASE64
        .decode(params.audio_base64.trim())
        .map_err(|error| Error::msg(format!("recorded audio could not be read: {error}")))?;
    if audio.is_empty() {
        return Err(Error::msg("No audio was recorded."));
    }
    if audio.len() > MAX_AUDIO_BYTES {
        return Err(Error::msg(
            "Recording is too long to transcribe (25 MB limit). Try a shorter dictation.",
        ));
    }
    let language = params
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    request_transcription(&base_url, &api_key, &audio, mime_type, extension, language)
}

fn request_transcription(
    base_url: &str,
    api_key: &str,
    audio: &[u8],
    mime_type: &str,
    extension: &str,
    language: Option<&str>,
) -> Result<String> {
    let file = reqwest::blocking::multipart::Part::bytes(audio.to_vec())
        .file_name(format!("dictation.{extension}"))
        .mime_str(mime_type)
        .map_err(|error| Error::msg(error.to_string()))?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .part("file", file)
        .text("model", TRANSCRIPTION_MODEL)
        .text("response_format", "json");
    if let Some(language) = language {
        form = form.text("language", language.to_owned());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|error| Error::msg(error.to_string()))?;
    let response = client
        .post(transcription_endpoint(base_url))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|error| {
            Error::msg(format!(
                "Could not reach the transcription provider: {error}"
            ))
        })?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| Error::msg(error.to_string()))?;
    if !status.is_success() {
        return Err(Error::msg(provider_error_message(status.as_u16(), &body)));
    }
    parse_transcript(&body)
}

fn transcription_endpoint(base_url: &str) -> String {
    format!("{}/audio/transcriptions", base_url.trim_end_matches('/'))
}

fn extension_for_mime(mime_type: &str) -> Result<&'static str> {
    match mime_type.to_ascii_lowercase().as_str() {
        "audio/webm" => Ok("webm"),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Ok("m4a"),
        "audio/ogg" => Ok("ogg"),
        "audio/wav" | "audio/x-wav" => Ok("wav"),
        "audio/mpeg" | "audio/mp3" => Ok("mp3"),
        "audio/flac" => Ok("flac"),
        other => Err(Error::msg(format!(
            "Unsupported recording format ({other})."
        ))),
    }
}

fn parse_transcript(body: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| {
        Error::msg(format!(
            "transcription provider returned invalid JSON: {error}"
        ))
    })?;
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| Error::msg("The transcription came back empty. Try speaking again."))?;
    Ok(text.to_owned())
}

fn provider_error_message(status: u16, body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .filter(|message| !message.trim().is_empty())
        {
            return format!("Transcription failed: {message}");
        }
    }
    format!("Transcription failed (provider returned HTTP {status}).")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploads_audio_with_private_auth_and_requested_model() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock provider");
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept request");
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = socket.read(&mut buffer).expect("read request");
                assert_ne!(read, 0, "request closed before upload completed");
                request.extend_from_slice(&buffer[..read]);
                if let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                {
                    let headers =
                        String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .expect("content length")
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-only-key"));
            assert!(request.contains("multipart/form-data; boundary="));
            assert!(request.contains("name=\"file\"; filename=\"dictation.webm\""));
            assert!(request.contains("sample audio bytes"));
            assert!(request.contains(TRANSCRIPTION_MODEL));
            assert!(request.contains("name=\"language\"\r\n\r\ntr"));
            let body = r#"{"text":"  dictated draft  "}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let result = request_transcription(
            &format!("http://{address}/v1"),
            "test-only-key",
            b"sample audio bytes",
            "audio/webm",
            "webm",
            Some("tr"),
        );
        server.join().expect("provider assertions");
        assert_eq!(result.expect("transcribe"), "dictated draft");
    }

    #[test]
    fn endpoint_appends_transcriptions_path() {
        assert_eq!(
            transcription_endpoint("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai/api/v1/audio/transcriptions"
        );
    }

    #[test]
    fn mime_types_map_to_upload_extensions() {
        assert_eq!(extension_for_mime("audio/webm").unwrap(), "webm");
        assert_eq!(extension_for_mime("audio/mp4").unwrap(), "m4a");
        assert!(extension_for_mime("video/mp4").is_err());
        assert!(extension_for_mime("").is_err());
    }

    #[test]
    fn transcript_text_is_trimmed_and_empty_is_rejected() {
        assert_eq!(
            parse_transcript(r#"{"text":"  hello  "}"#).unwrap(),
            "hello"
        );
        assert!(parse_transcript(r#"{"text":"   "}"#).is_err());
        assert!(parse_transcript(r#"{}"#).is_err());
    }

    #[test]
    fn provider_error_message_prefers_provider_detail() {
        let message = provider_error_message(401, r#"{"error":{"message":"bad key"}}"#);
        assert!(message.contains("bad key"), "{message}");
        assert!(provider_error_message(500, "oops").contains("500"));
    }
}
