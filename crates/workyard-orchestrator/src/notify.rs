//! Desktop notifications for things a human has to act on.

pub trait Notifier: Send {
    fn notify(&self, title: &str, body: &str);
}

/// Discards notifications (tests, or `--no-notify`).
pub struct Silent;

impl Notifier for Silent {
    fn notify(&self, _title: &str, _body: &str) {}
}

/// macOS notification through `osascript`. It adds no dependency and works with the desktop
/// app closed, since it comes from the orchestrator.
pub struct MacNotifier;

fn applescript_string(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    format!("\"{}\"", flat.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Notifier for MacNotifier {
    fn notify(&self, title: &str, body: &str) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let script = format!(
            "display notification {} with title {}",
            applescript_string(body),
            applescript_string(title)
        );
        if let Ok(mut child) = std::process::Command::new("osascript").args(["-e", &script]).spawn() {
            // Reap it without blocking the loop.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::applescript_string;

    #[test]
    fn quotes_and_newlines_cannot_break_out_of_the_script() {
        assert_eq!(applescript_string("say \"hi\"\nnow\\"), "\"say \\\"hi\\\" now\\\\\"");
    }
}
