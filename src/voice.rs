//! Speech for the speech instance: record from the microphone (PipeWire
//! `pw-record`, else `arecord`), transcribe with the voice provider, speak
//! replies. Providers (config `voice`): `openai` kind (OpenAI, DeepInfra,
//! OpenRouter: `/audio/transcriptions`, `/audio/speech`) or `elevenlabs`.

use crate::config::Voice;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub fn dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ventosa").join("speech")
}

/// A running microphone recording.
pub struct Recording {
    child: Child,
    pub path: PathBuf,
    pub started: std::time::Instant,
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// Start recording 16 kHz mono WAV.
pub fn record() -> Result<Recording, String> {
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let path = dir().join(format!("in-{}.wav", crate::now()));
    let child = if on_path("pw-record") {
        Command::new("pw-record").args(["--rate", "16000", "--channels", "1", "--format", "s16"]).arg(&path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    } else if on_path("arecord") {
        Command::new("arecord").args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1"]).arg(&path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    } else {
        return Err("no recorder: install pipewire (pw-record) or alsa-utils (arecord)".into());
    }
    .map_err(|e| e.to_string())?;
    Ok(Recording { child, path, started: std::time::Instant::now() })
}

impl Recording {
    /// Stop cleanly (SIGINT lets the recorder finish the WAV header) and return the file.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        for _ in 0..30 {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let len = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if len < 4000 {
            return Err("recording is empty or too short".into());
        }
        Ok(self.path)
    }
}

fn key(v: &Voice) -> Result<String, String> {
    std::env::var(&v.api_key_env).ok().filter(|k| !k.is_empty()).ok_or_else(|| format!("environment variable {} is not set (settings ▸ voice)", v.api_key_env))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(180)).build()
}

fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => format!("HTTP {code}: {}", r.into_string().unwrap_or_default()),
        other => other.to_string(),
    }
}

/// multipart/form-data body with text fields and one file.
fn multipart(fields: &[(&str, &str)], file_field: &str, file: &Path) -> Result<(String, Vec<u8>), String> {
    let boundary = format!("ventosa{}", crate::now());
    let mut b: Vec<u8> = vec![];
    for (k, v) in fields {
        b.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("audio.wav");
    b.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{name}\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    b.extend(std::fs::read(file).map_err(|e| e.to_string())?);
    b.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok((format!("multipart/form-data; boundary={boundary}"), b))
}

/// Speech → text.
pub fn transcribe(v: &Voice, audio: &Path) -> Result<String, String> {
    let k = key(v)?;
    let base = v.base_url.trim_end_matches('/');
    let resp: Value = match v.kind.as_str() {
        "elevenlabs" => {
            let (ct, body) = multipart(&[("model_id", &v.stt_model)], "file", audio)?;
            agent().post(&format!("{base}/v1/speech-to-text")).set("xi-api-key", &k).set("content-type", &ct).send_bytes(&body).map_err(http_err)?.into_json().map_err(|e| e.to_string())?
        }
        _ => {
            let (ct, body) = multipart(&[("model", &v.stt_model)], "file", audio)?;
            agent().post(&format!("{base}/audio/transcriptions")).set("authorization", &format!("Bearer {k}")).set("content-type", &ct).send_bytes(&body).map_err(http_err)?.into_json().map_err(|e| e.to_string())?
        }
    };
    resp["text"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or_else(|| format!("no text in reply: {resp}"))
}

/// Text → speech file (mp3).
pub fn synthesize(v: &Voice, text: &str) -> Result<PathBuf, String> {
    let k = key(v)?;
    let base = v.base_url.trim_end_matches('/');
    let text: String = text.chars().take(4000).collect();
    let resp = match v.kind.as_str() {
        "elevenlabs" => agent()
            .post(&format!("{base}/v1/text-to-speech/{}", v.voice))
            .set("xi-api-key", &k)
            .set("accept", "audio/mpeg")
            .send_json(json!({"text": text, "model_id": v.tts_model}))
            .map_err(http_err)?,
        _ => agent()
            .post(&format!("{base}/audio/speech"))
            .set("authorization", &format!("Bearer {k}"))
            .send_json(json!({"model": v.tts_model, "voice": v.voice, "input": text, "response_format": "mp3"}))
            .map_err(http_err)?,
    };
    let mut bytes = vec![];
    std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let out = dir().join(format!("out-{}.mp3", crate::now()));
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Play an audio file without a window (mpv, else ffplay). Returns at once.
pub fn play(path: &Path) -> Result<Child, String> {
    let mut c = if on_path("mpv") {
        let mut c = Command::new("mpv");
        c.args(["--no-video", "--really-quiet"]);
        c
    } else {
        let mut c = Command::new("ffplay");
        c.args(["-nodisp", "-autoexit", "-loglevel", "quiet"]);
        c
    };
    c.arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())
}
